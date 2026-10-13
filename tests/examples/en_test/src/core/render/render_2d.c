#include "../internal.h"
#include "render/internal.h"
#include "types.h"
#include "en_components.h"

#define STB_IMAGE_IMPLEMENTATION
#include "stb_image.h"

#include <stdlib.h>
#include <string.h>
#include <math.h>

static EnCamera2D* g_found_camera = NULL;

// ============================================================================
// Texture
// ============================================================================

struct EnTexture {
    WGPUTexture     texture;
    WGPUTextureView view;
    WGPUSampler     sampler;
    WGPUBindGroup   bind_group;
    u32             width;
    u32             height;
};

static WGPUBindGroup make_texture_bind_group(EnRenderer* r,
                                             WGPUTextureView view,
                                             WGPUSampler sampler) {
    WGPUBindGroupEntry entries[2] = {0};
    entries[0].binding = 0;
    entries[0].textureView = view;
    entries[1].binding = 1;
    entries[1].sampler = sampler;

    WGPUBindGroupDescriptor desc = {0};
    desc.layout     = r->r2d.texture_bgl;
    desc.entryCount = 2;
    desc.entries    = entries;
    return wgpuDeviceCreateBindGroup(r->device, &desc);
}

static EnTexture* create_texture_from_pixels(EnEngine* engine,
                                             const u8* rgba, u32 w, u32 h) {
    EnRenderer* r = engine->renderer;

    EnTexture* t = calloc(1, sizeof(EnTexture));
    t->width = w; t->height = h;

    WGPUTextureDescriptor td = {0};
    td.size.width  = w;
    td.size.height = h;
    td.size.depthOrArrayLayers = 1;
    td.format     = WGPUTextureFormat_RGBA8UnormSrgb;
    td.usage      = WGPUTextureUsage_TextureBinding | WGPUTextureUsage_CopyDst;
    td.dimension  = WGPUTextureDimension_2D;
    td.mipLevelCount = 1;
    td.sampleCount   = 1;
    t->texture = wgpuDeviceCreateTexture(r->device, &td);

    WGPUTexelCopyTextureInfo dst = {0};
    dst.texture  = t->texture;
    dst.mipLevel = 0;
    dst.origin   = (WGPUOrigin3D){0, 0, 0};
    dst.aspect   = WGPUTextureAspect_All;

    WGPUTexelCopyBufferLayout layout = {0};
    layout.bytesPerRow  = w * 4;
    layout.rowsPerImage = h;

    WGPUExtent3D extent = { .width = w, .height = h, .depthOrArrayLayers = 1 };
    wgpuQueueWriteTexture(r->queue, &dst, rgba, w * h * 4, &layout, &extent);

    t->view = wgpuTextureCreateView(t->texture, NULL);

    WGPUSamplerDescriptor sd = {0};
    sd.addressModeU  = WGPUAddressMode_ClampToEdge;
    sd.addressModeV  = WGPUAddressMode_ClampToEdge;
    sd.addressModeW  = WGPUAddressMode_ClampToEdge;
    sd.magFilter     = WGPUFilterMode_Nearest;
    sd.minFilter     = WGPUFilterMode_Nearest;
    sd.mipmapFilter  = WGPUMipmapFilterMode_Nearest;
    sd.lodMinClamp   = 0.0f;
    sd.lodMaxClamp   = 32.0f;
    sd.maxAnisotropy = 1;
    t->sampler = wgpuDeviceCreateSampler(r->device, &sd);

    t->bind_group = make_texture_bind_group(r, t->view, t->sampler);
    return t;
}

EnTexture* en_render_2d_texture_load(EnEngine* engine, const char* path) {
    if (!engine || !path) return NULL;
    EnAssetId id = en_asset_load(engine, path);
    if (id == EN_ASSET_INVALID) return NULL;
    return (EnTexture*)en_asset_data(engine, id);
}

EnTexture* en_render_2d_texture_white(EnEngine* engine) {
    if (!engine || !engine->renderer) return NULL;
    if (engine->renderer->r2d.white) return engine->renderer->r2d.white;

    u8 px[4] = { 255, 255, 255, 255 };
    engine->renderer->r2d.white = create_texture_from_pixels(engine, px, 1, 1);
    return engine->renderer->r2d.white;
}

void en_render_2d_texture_free(EnEngine* engine, EnTexture* tex) {
    if (!engine || !engine->renderer || !tex) return;
    if (tex == engine->renderer->r2d.white) return;  // don't free fallback

    wgpuBindGroupRelease(tex->bind_group);
    wgpuSamplerRelease(tex->sampler);
    wgpuTextureViewRelease(tex->view);
    wgpuTextureRelease(tex->texture);
    free(tex);
}

u32 en_render_2d_texture_width(const EnTexture* tex)  { return tex ? tex->width : 0; }
u32 en_render_2d_texture_height(const EnTexture* tex) { return tex ? tex->height : 0; }

static void* png_asset_load(EnEngine* engine, const char* path,
                            const u8* bytes, usize size, void* ud) {
    (void)path; (void)ud;

    int w, h, ch;
    stbi_uc* pixels = stbi_load_from_memory(bytes, (int)size, &w, &h, &ch, 4);
    if (!pixels) {
        en_error("PNG decode failed: %s", stbi_failure_reason());
        return NULL;
    }

    EnTexture* t = create_texture_from_pixels(engine, pixels, (u32)w, (u32)h);
    stbi_image_free(pixels);
    return t;
}

static void png_asset_free(EnEngine* engine, void* asset, void* ud) {
    (void)ud;
    en_render_2d_texture_free(engine, (EnTexture*)asset);
}

// ============================================================================
// Shader (embedded WGSL)
// ============================================================================

static const char SHADER_2D_WGSL[] = {
    #embed "shaders/shader_2d.wgsl"
    , 0    // explicit NUL for WGPU_STRLEN
};

// ============================================================================
// Init / shutdown
// ============================================================================

bool en_render_2d_init(EnEngine* engine) {
    if (!engine || !engine->renderer) return false;
    EnRenderer* r = engine->renderer;
    EnRender2D* r2 = &r->r2d;

    // --- Shader module ---
    WGPUShaderSourceWGSL wgsl = {0};
    wgsl.chain.sType = WGPUSType_ShaderSourceWGSL;
    wgsl.code.data   = SHADER_2D_WGSL;
    wgsl.code.length = WGPU_STRLEN;

    WGPUShaderModuleDescriptor smd = {0};
    smd.nextInChain = &wgsl.chain;
    WGPUShaderModule shader = wgpuDeviceCreateShaderModule(r->device, &smd);
    if (!shader) { en_error("2D: shader module failed"); return false; }

    // --- Bind group layouts ---
    WGPUBindGroupLayoutEntry cam_e = {0};
    cam_e.binding = 0;
    cam_e.visibility = WGPUShaderStage_Vertex;
    cam_e.buffer.type = WGPUBufferBindingType_Uniform;
    WGPUBindGroupLayoutDescriptor bgl0d = {0};
    bgl0d.entryCount = 1;
    bgl0d.entries = &cam_e;
    r2->uniform_bgl = wgpuDeviceCreateBindGroupLayout(r->device, &bgl0d);

    WGPUBindGroupLayoutEntry tex_e = {0};
    tex_e.binding = 0;
    tex_e.visibility = WGPUShaderStage_Fragment;
    tex_e.texture.sampleType = WGPUTextureSampleType_Float;
    tex_e.texture.viewDimension = WGPUTextureViewDimension_2D;
    WGPUBindGroupLayoutEntry smp_e = {0};
    smp_e.binding = 1;
    smp_e.visibility = WGPUShaderStage_Fragment;
    smp_e.sampler.type = WGPUSamplerBindingType_Filtering;
    WGPUBindGroupLayoutEntry tex_entries[2] = { tex_e, smp_e };
    WGPUBindGroupLayoutDescriptor bgl1d = {0};
    bgl1d.entryCount = 2;
    bgl1d.entries = tex_entries;
    r2->texture_bgl = wgpuDeviceCreateBindGroupLayout(r->device, &bgl1d);

    WGPUBindGroupLayout bgls[2] = { r2->uniform_bgl, r2->texture_bgl };
    WGPUPipelineLayoutDescriptor pld = {0};
    pld.bindGroupLayoutCount = 2;
    pld.bindGroupLayouts = bgls;
    WGPUPipelineLayout pl = wgpuDeviceCreatePipelineLayout(r->device, &pld);

    // --- Vertex layout ---
    WGPUVertexAttribute vattrs[2] = {
        { .format = WGPUVertexFormat_Float32x2, .offset = 0, .shaderLocation = 0 },
        { .format = WGPUVertexFormat_Float32x2, .offset = 8, .shaderLocation = 1 },
    };
    WGPUVertexAttribute iattrs[4] = {
        { .format = WGPUVertexFormat_Float32x4, .offset = 0,  .shaderLocation = 2 },
        { .format = WGPUVertexFormat_Float32x4, .offset = 16, .shaderLocation = 3 },
        { .format = WGPUVertexFormat_Float32x2, .offset = 32, .shaderLocation = 4 },
    };
    // Wait, instance has 4 attributes: transform, color, cos_rot+sin_rot.
    // Total 3 attributes actually (transform, color, rot) — but rot is vec2 at offset 32.
    // Let me redo:
    WGPUVertexAttribute iattrs2[3] = {
        { .format = WGPUVertexFormat_Float32x4, .offset = 0,  .shaderLocation = 2 },  // transform
        { .format = WGPUVertexFormat_Float32x4, .offset = 16, .shaderLocation = 3 },  // color
        { .format = WGPUVertexFormat_Float32x2, .offset = 32, .shaderLocation = 4 },  // rot
    };

    WGPUVertexBufferLayout vbls[2] = {
        {
            .arrayStride = 16,  // sizeof(Vertex) = 8 + 8
            .stepMode = WGPUVertexStepMode_Vertex,
            .attributeCount = 2,
            .attributes = vattrs,
        },
        {
            .arrayStride = sizeof(EnSpriteInstance),  // 48
            .stepMode = WGPUVertexStepMode_Instance,
            .attributeCount = 3,
            .attributes = iattrs2,
        },
    };

    WGPUVertexState vs = {0};
    vs.module = shader;
    vs.entryPoint.data = "vs_main";
    vs.entryPoint.length = WGPU_STRLEN;
    vs.bufferCount = 2;
    vs.buffers = vbls;

    WGPUPrimitiveState prim = {0};
    prim.topology = WGPUPrimitiveTopology_TriangleList;
    prim.cullMode = WGPUCullMode_None;

    WGPUBlendState blend = {0};
    blend.color.srcFactor = WGPUBlendFactor_SrcAlpha;
    blend.color.dstFactor = WGPUBlendFactor_OneMinusSrcAlpha;
    blend.color.operation = WGPUBlendOperation_Add;
    blend.alpha.srcFactor = WGPUBlendFactor_One;
    blend.alpha.dstFactor = WGPUBlendFactor_OneMinusSrcAlpha;
    blend.alpha.operation = WGPUBlendOperation_Add;

    WGPUColorTargetState ct = {0};
    ct.format = r->surface_format;
    ct.blend = &blend;
    ct.writeMask = WGPUColorWriteMask_All;

    WGPUFragmentState fs = {0};
    fs.module = shader;
    fs.entryPoint.data = "fs_main";
    fs.entryPoint.length = WGPU_STRLEN;
    fs.targetCount = 1;
    fs.targets = &ct;

    WGPURenderPipelineDescriptor pd = {0};
    pd.layout = pl;
    pd.vertex = vs;
    pd.primitive = prim;
    pd.fragment = &fs;
    pd.multisample.count = 1;
    pd.multisample.mask = 0xFFFFFFFF;
    r2->pipeline = wgpuDeviceCreateRenderPipeline(r->device, &pd);

    wgpuShaderModuleRelease(shader);
    wgpuPipelineLayoutRelease(pl);

    // --- Camera uniform buffer (64 bytes for mat4) ---
    WGPUBufferDescriptor cbd = {0};
    cbd.size = 64;
    cbd.usage = WGPUBufferUsage_Uniform | WGPUBufferUsage_CopyDst;
    r2->camera_buffer = wgpuDeviceCreateBuffer(r->device, &cbd);

    WGPUBindGroupEntry cbe = {0};
    cbe.binding = 0;
    cbe.buffer = r2->camera_buffer;
    cbe.size   = 64;
    WGPUBindGroupDescriptor cbgd = {0};
    cbgd.layout = r2->uniform_bgl;
    cbgd.entryCount = 1;
    cbgd.entries = &cbe;
    r2->uniform_bg = wgpuDeviceCreateBindGroup(r->device, &cbgd);

    // --- Quad vertex buffer (4 verts) ---
    typedef struct { f32 x, y, u, v; } V;
    V verts[4] = {
        { -0.5f, -0.5f, 0.0f, 1.0f },
        {  0.5f, -0.5f, 1.0f, 1.0f },
        {  0.5f,  0.5f, 1.0f, 0.0f },
        { -0.5f,  0.5f, 0.0f, 0.0f },
    };
    u16 idx[6] = { 0, 1, 2, 0, 2, 3 };

    WGPUBufferDescriptor vbd = {0};
    vbd.size  = sizeof(verts);
    vbd.usage = WGPUBufferUsage_Vertex | WGPUBufferUsage_CopyDst;
    r2->vertex_buffer = wgpuDeviceCreateBuffer(r->device, &vbd);
    wgpuQueueWriteBuffer(r->queue, r2->vertex_buffer, 0, verts, sizeof(verts));

    WGPUBufferDescriptor ibd = {0};
    ibd.size  = sizeof(idx);
    ibd.usage = WGPUBufferUsage_Index | WGPUBufferUsage_CopyDst;
    r2->index_buffer = wgpuDeviceCreateBuffer(r->device, &ibd);
    wgpuQueueWriteBuffer(r->queue, r2->index_buffer, 0, idx, sizeof(idx));

    // --- Instance buffer (grow on demand) ---
    r2->instance_capacity = 256;
    r2->cpu_capacity = 256;
    r2->cpu_instances = calloc(r2->cpu_capacity, sizeof(EnSpriteInstance));

    WGPUBufferDescriptor insd = {0};
    insd.size  = r2->instance_capacity * sizeof(EnSpriteInstance);
    insd.usage = WGPUBufferUsage_Vertex | WGPUBufferUsage_CopyDst;
    r2->instance_buffer = wgpuDeviceCreateBuffer(r->device, &insd);

    // --- Default camera ---
    r2->camera.x = 0.0f;
    r2->camera.y = 0.0f;
    r2->camera.zoom = 1.0f;

    // --- Default white texture ---
    en_render_2d_texture_white(engine);

    EnAssetHandler png = {
        .extension = "png",
        .load      = png_asset_load,
        .free      = png_asset_free,
        .user_data = NULL,
    };
    en_register_asset_handler(engine, &png);

    en_log("Render 2D initialized");
    return true;
}

void en_render_2d_shutdown(EnEngine* engine) {
    if (!engine || !engine->renderer) return;
    EnRenderer* r = engine->renderer;
    EnRender2D* r2 = &r->r2d;

    if (r2->white) {
        wgpuBindGroupRelease(r2->white->bind_group);
        wgpuSamplerRelease(r2->white->sampler);
        wgpuTextureViewRelease(r2->white->view);
        wgpuTextureRelease(r2->white->texture);
        free(r2->white);
        r2->white = NULL;
    }
    if (r2->instance_buffer) wgpuBufferRelease(r2->instance_buffer);
    if (r2->index_buffer)    wgpuBufferRelease(r2->index_buffer);
    if (r2->vertex_buffer)   wgpuBufferRelease(r2->vertex_buffer);
    if (r2->camera_buffer)   wgpuBufferRelease(r2->camera_buffer);
    if (r2->uniform_bg)      wgpuBindGroupRelease(r2->uniform_bg);
    if (r2->pipeline)        wgpuRenderPipelineRelease(r2->pipeline);
    if (r2->texture_bgl)     wgpuBindGroupLayoutRelease(r2->texture_bgl);
    if (r2->uniform_bgl)     wgpuBindGroupLayoutRelease(r2->uniform_bgl);
    free(r2->cpu_instances);
    r2->cpu_instances = NULL;

    en_log("Render 2D shut down");
}

// ============================================================================
// Camera
// ============================================================================

void en_render_2d_set_camera(EnEngine* engine, EnCamera2D cam) {
    if (!engine || !engine->renderer) return;
    engine->renderer->r2d.camera = cam;
}

static void upload_camera(EnEngine* engine) {
    EnRenderer* r = engine->renderer;
    EnRender2D* r2 = &r->r2d;

    f32 W = (f32)r->width;
    f32 H = (f32)r->height;
    f32 Z = r2->camera.zoom;
    f32 cx = r2->camera.x;
    f32 cy = r2->camera.y;

    if (W <= 0) W = 1;
    if (H <= 0) H = 1;

    // Column-major mat4: maps world (x, y, 0, 1) to NDC.
    // x_ndc = 2Z/W * x - 2Z*cx/W
    // y_ndc = 2Z/H * y - 2Z*cy/H
    f32 m[16] = {
        2.0f * Z / W, 0, 0, 0,
        0, 2.0f * Z / H, 0, 0,
        0, 0, 1, 0,
        -2.0f * Z * cx / W, -2.0f * Z * cy / H, 0, 1,
    };

    wgpuQueueWriteBuffer(r->queue, r2->camera_buffer, 0, m, sizeof(m));
}

// ============================================================================
// Draw commands
// ============================================================================

// Batched call: add N sprites with the same texture, no per-call overhead.
void en_render_2d_begin(EnEngine* engine) {
    if (!engine || !engine->renderer) return;
    EnRender2D* r2 = &engine->renderer->r2d;

    r2->instance_count = 0;
    r2->batch_count = 0;
    r2->current_texture = NULL;
    r2->current_batch_count = 0;
}

static void push_batch(EnRender2D* r2, EnTexture* tex,
                       u32 first, u32 count) {
    if (r2->batch_count >= r2->batch_capacity) {
        usize nc = r2->batch_capacity ? r2->batch_capacity * 2 : 16;
        r2->batches = realloc(r2->batches, nc * sizeof(EnSpriteBatch));
        if (!r2->batches) {
            en_error("push_batch: realloc failed");
            return;
        }
        r2->batch_capacity = nc;
    }
    EnSpriteBatch* b = &r2->batches[r2->batch_count++];
    b->texture = tex;
    b->first   = first;
    b->count   = count;
}

static void add_instance(EnRender2D* r2, Instance2D s) {
    if (r2->instance_count >= r2->cpu_capacity) {
        usize nc = r2->cpu_capacity * 2;
        r2->cpu_instances = realloc(r2->cpu_instances, nc * sizeof(EnSpriteInstance));
        r2->cpu_capacity = nc;
    }
    EnSpriteInstance* inst = &r2->cpu_instances[r2->instance_count++];
    inst->tx = s.x; inst->ty = s.y;
    inst->tw = s.w; inst->th = s.h;
    inst->r = s.r; inst->g = s.g; inst->b = s.b; inst->a = s.a;
    inst->cos_rot = cosf(s.rotation);
    inst->sin_rot = sinf(s.rotation);
    inst->_pad[0] = inst->_pad[1] = 0.0f;
}

void en_render_2d_draw(EnEngine* engine, const EnTexture* tex, Instance2D s) {
    if (!engine || !engine->renderer) return;
    EnRender2D* r2 = &engine->renderer->r2d;

    EnTexture* t = (EnTexture*)tex;
    if (!t) t = r2->white;

    // If texture differs from current batch — close current, open new.
    if (r2->current_texture != t) {
        if (r2->current_batch_count > 0) {
            push_batch(r2, r2->current_texture,
                       r2->current_batch_first, r2->current_batch_count);
        }
        r2->current_texture     = t;
        r2->current_batch_first = (u32)r2->instance_count;
        r2->current_batch_count = 0;
    }

    add_instance(r2, s);
    r2->current_batch_count++;
}

static void draw_chunk_cb(EnEntity* ents, u8** cols, usize n, void* ud) {
    (void)ents;
    EnEngine* engine = ud;
    EnTransform2D* tr = (EnTransform2D*)cols[0];
    EnSprite2D*    sp = (EnSprite2D*)cols[1];

    for (usize i = 0; i < n; i++) {
        EnTexture* tex = NULL;
        if (sp[i].texture != EN_ASSET_INVALID)
            tex = (EnTexture*)en_asset_data(engine, sp[i].texture);

        Instance2D cmd = {
            .x = tr[i].x, .y = tr[i].y,
            .w = sp[i].w  * tr[i].scale_x,
            .h = sp[i].h * tr[i].scale_y,
            .rotation = tr[i].rotation,
            .r = sp[i].r, .g = sp[i].g, .b = sp[i].b, .a = sp[i].a,
        };
        en_render_2d_draw(engine, tex, cmd);
    }
}

static void find_camera_cb(EnEntity* ents, u8** cols, usize n, void* ud) {
    (void)ents; (void)ud;
    if (n > 0 && !g_found_camera) {
        g_found_camera = (EnCamera2D*)cols[0];
    }
}

void en_render_2d_render_world(EnEngine* engine, WGPURenderPassEncoder pass) {
    if (!engine || !engine->renderer || !engine->world) return;

    // Find camera entity (first one wins).
    g_found_camera = NULL;
    u32 cam_ids[1] = { engine->comp_camera2d };
    en_world_query(engine->world, cam_ids, 1, find_camera_cb, NULL);

    if (g_found_camera) {
        en_render_2d_set_camera(engine, *g_found_camera);
    } else {
        // No camera in scene — default so things are visible.
        EnCamera2D def = { .x = 0, .y = 0, .zoom = 64.0f };
        en_render_2d_set_camera(engine, def);
    }

    en_render_2d_begin(engine);

    u32 ids[2] = { engine->comp_transform2d, engine->comp_sprite2d };
    en_world_query(engine->world, ids, 2, draw_chunk_cb, engine);

    en_render_2d_end(engine);
    en_render_2d_flush(engine, pass);
}

void en_render_2d_end(EnEngine* engine) {
    if (!engine || !engine->renderer) return;
    EnRender2D* r2 = &engine->renderer->r2d;
    if (r2->current_batch_count > 0) {
        push_batch(r2, r2->current_texture,
                   r2->current_batch_first, r2->current_batch_count);
        r2->current_batch_count = 0;
    }
}

// ============================================================================
// Frame integration — called by render.c
// ============================================================================

void en_render_2d_flush(EnEngine* engine, WGPURenderPassEncoder pass) {
    if (!engine || !engine->renderer) return;
    EnRenderer* r = engine->renderer;
    EnRender2D* r2 = &r->r2d;

    if (r2->instance_count == 0) return;

    // Upload camera + all instances
    upload_camera(engine);
    wgpuQueueWriteBuffer(r->queue, r2->instance_buffer, 0,
                         r2->cpu_instances,
                         r2->instance_count * sizeof(EnSpriteInstance));

    wgpuRenderPassEncoderSetPipeline(pass, r2->pipeline);
    wgpuRenderPassEncoderSetBindGroup(pass, 0, r2->uniform_bg, 0, NULL);
    wgpuRenderPassEncoderSetVertexBuffer(pass, 0, r2->vertex_buffer, 0, 64);
    wgpuRenderPassEncoderSetVertexBuffer(pass, 1, r2->instance_buffer, 0,
                                         r2->instance_count * sizeof(EnSpriteInstance));
    wgpuRenderPassEncoderSetIndexBuffer(pass, r2->index_buffer,
                                        WGPUIndexFormat_Uint16, 0, 12);

    for (usize i = 0; i < r2->batch_count; i++) {
        EnSpriteBatch* b = &r2->batches[i];
        wgpuRenderPassEncoderSetBindGroup(pass, 1, b->texture->bind_group, 0, NULL);
        wgpuRenderPassEncoderDrawIndexed(pass, 6, b->count, 0, 0, b->first);
    }
}
