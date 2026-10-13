#ifndef EN_RENDER_INTERNAL_H
#define EN_RENDER_INTERNAL_H

#include "en_render_2d.h"
#include <webgpu/wgpu.h>

// ============================================================================
// 2D renderer internals
// ============================================================================

typedef struct {
    EnTexture* texture;      // NULL = white
    u32        first;        // index of first instance in cpu_instances
    u32        count;        // how many instances in this batch
} EnSpriteBatch;

typedef struct {
    f32 tx, ty, tw, th;   // 16
    f32 r, g, b, a;       // 16
    f32 cos_rot, sin_rot; // 8
    f32 _pad[2];          // 8 → total 48
} EnSpriteInstance;

typedef struct EnRender2D {
    WGPURenderPipeline pipeline;
    WGPUBindGroupLayout uniform_bgl;
    WGPUBindGroupLayout texture_bgl;
    WGPUBindGroup uniform_bg;
    WGPUBuffer camera_buffer;

    WGPUBuffer vertex_buffer;
    WGPUBuffer index_buffer;

    WGPUBuffer instance_buffer;
    usize      instance_capacity;
    usize      instance_count;

    EnSpriteInstance* cpu_instances;
    usize             cpu_capacity;

    EnTexture* white;               // fallback
    EnTexture* current_texture;
    u32        current_batch_first;
    u32        current_batch_count;

    EnSpriteBatch* batches;
    usize          batch_count;
    usize          batch_capacity;

    EnCamera2D camera;
} EnRender2D;

// Extended renderer — put this INSIDE EnRenderer as field `EnRender2D r2d;`
// (edit the struct above to add it)

bool en_render_2d_init    (EnEngine* engine);
void en_render_2d_shutdown(EnEngine* engine);
void en_render_2d_flush  (EnEngine* engine, WGPURenderPassEncoder pass);

typedef struct EnRenderer {
    WGPUInstance instance;
    WGPUSurface  surface;
    WGPUAdapter  adapter;
    WGPUDevice   device;
    WGPUQueue    queue;

    WGPUTextureFormat surface_format;
    u32 width;
    u32 height;

    WGPUColor clear_color;
    EnRender2D r2d;
} EnRenderer;

bool en_render_init(EnEngine* engine);
void en_render_frame(EnEngine* engine);
void en_render_shutdown(EnEngine* engine);
void en_render_resize(EnEngine* engine, u32 w, u32 h);

void en_render_2d_render_world(EnEngine* engine, WGPURenderPassEncoder pass);
void en_render_2d_flush(EnEngine* engine, WGPURenderPassEncoder pass);

#endif // EN_RENDER_INTERNAL_H
