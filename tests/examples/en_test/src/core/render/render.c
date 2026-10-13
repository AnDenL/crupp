#include "render/internal.h"
#include "window/internal.h"


#include <stdlib.h>
#include <string.h>

// ============================================================================
// Sync helpers — wgpu-native uses async callbacks, we block until done
// ============================================================================

typedef struct { WGPUAdapter adapter; bool done; } AdapterResult;
typedef struct { WGPUDevice  device;  bool done; } DeviceResult;

static void on_adapter(WGPURequestAdapterStatus status, WGPUAdapter adapter,
                       WGPUStringView msg, void* u1, void* u2) {
    (void)u2;
    AdapterResult* r = u1;
    if (status == WGPURequestAdapterStatus_Success) {
        r->adapter = adapter;
    } else {
        en_error("Request adapter failed: %.*s", (int)msg.length, msg.data ? msg.data : "");
    }
    r->done = true;
}

static void on_device(WGPURequestDeviceStatus status, WGPUDevice device,
                      WGPUStringView msg, void* u1, void* u2) {
    (void)u2;
    DeviceResult* r = u1;
    if (status == WGPURequestDeviceStatus_Success) {
        r->device = device;
    } else {
        en_error("Request device failed: %.*s", (int)msg.length, msg.data ? msg.data : "");
    }
    r->done = true;
}

static void on_device_lost(WGPUDevice const* device, WGPUDeviceLostReason reason,
                           WGPUStringView msg, void* u1, void* u2) {
    (void)device; (void)reason; (void)u1; (void)u2;
    en_warn("Device lost: %.*s", (int)msg.length, msg.data ? msg.data : "");
}

static void on_uncaptured_error(WGPUDevice const* device, WGPUErrorType type,
                                WGPUStringView msg, void* u1, void* u2) {
    (void)device; (void)u1; (void)u2;
    en_error("wgpu error type=%d: %.*s", (int)type,
             (int)msg.length, msg.data ? msg.data : "");
}

// ============================================================================
// Surface creation per platform
// ============================================================================

static WGPUSurface create_surface(WGPUInstance instance, void* window) {
    EnWindowPlatform plat = en_window_platform(window);

    if (plat == EN_WINDOW_PLATFORM_WAYLAND) {
        void* display = en_window_wayland_display(window);
        void* surface = en_window_wayland_surface(window);
        if (!display || !surface) {
            en_error("Wayland display/surface is NULL");
            return NULL;
        }

        WGPUSurfaceSourceWaylandSurface src = {0};
        src.chain.sType = WGPUSType_SurfaceSourceWaylandSurface;
        src.display = display;
        src.surface = surface;

        WGPUSurfaceDescriptor desc = {0};
        desc.nextInChain = &src.chain;
        return wgpuInstanceCreateSurface(instance, &desc);
    }

    if (plat == EN_WINDOW_PLATFORM_X11) {
        void* display = en_window_x11_display(window);
        u64   xwin    = en_window_x11_window(window);

        WGPUSurfaceSourceXlibWindow src = {0};
        src.chain.sType = WGPUSType_SurfaceSourceXlibWindow;
        src.display = display;
        src.window  = xwin;

        WGPUSurfaceDescriptor desc = {0};
        desc.nextInChain = &src.chain;
        return wgpuInstanceCreateSurface(instance, &desc);
    }

    en_error("Unsupported window platform (neither Wayland nor X11)");
    return NULL;
}

// ============================================================================
// Init / shutdown
// ============================================================================

bool en_render_init(EnEngine* engine) {
    if (!engine || !engine->window) {
        en_error("Render: no window");
        return false;
    }

    EnRenderer* r = calloc(1, sizeof(EnRenderer));
    if (!r) return false;

    // Default clear color
    r->clear_color = (WGPUColor){ 0.05, 0.07, 0.10, 1.0 };

    // 1. Instance
    r->instance = wgpuCreateInstance(NULL);
    if (!r->instance) {
        en_error("Failed to create wgpu instance");
        free(r);
        return false;
    }

    // 2. Surface from window
    r->surface = create_surface(r->instance, engine->window);
    if (!r->surface) {
        wgpuInstanceRelease(r->instance);
        free(r);
        return false;
    }

    // 3. Adapter
    AdapterResult ar = {0};
    WGPURequestAdapterOptions aopts = {0};
    aopts.compatibleSurface = r->surface;
    aopts.powerPreference   = WGPUPowerPreference_HighPerformance;

    WGPURequestAdapterCallbackInfo acb = {0};
    acb.mode      = WGPUCallbackMode_WaitAnyOnly;
    acb.callback  = on_adapter;
    acb.userdata1 = &ar;
    wgpuInstanceRequestAdapter(r->instance, &aopts, acb);

    while (!ar.done) {
        wgpuInstanceProcessEvents(r->instance);
    }
    r->adapter = ar.adapter;
    if (!r->adapter) {
        en_error("No suitable adapter");
        en_render_shutdown(engine);
        free(r);
        return false;
    }

    // 4. Device
    DeviceResult dr = {0};

    WGPUDeviceDescriptor ddesc = {0};
    ddesc.deviceLostCallbackInfo.callback = on_device_lost;
    ddesc.uncapturedErrorCallbackInfo.callback = on_uncaptured_error;

    WGPURequestDeviceCallbackInfo dcb = {0};
    dcb.mode      = WGPUCallbackMode_WaitAnyOnly;
    dcb.callback  = on_device;
    dcb.userdata1 = &dr;
    wgpuAdapterRequestDevice(r->adapter, &ddesc, dcb);

    while (!dr.done) {
        wgpuInstanceProcessEvents(r->instance);
    }
    r->device = dr.device;
    if (!r->device) {
        en_error("No device");
        en_render_shutdown(engine);
        free(r);
        return false;
    }

    r->queue = wgpuDeviceGetQueue(r->device);

    // 5. Surface config
    en_window_size(engine->window, &r->width, &r->height);
    if (r->width == 0) r->width = 1280;
    if (r->height == 0) r->height = 720;

    WGPUSurfaceCapabilities caps = {0};
    wgpuSurfaceGetCapabilities(r->surface, r->adapter, &caps);
    r->surface_format = caps.formats[0];

    WGPUSurfaceConfiguration scfg = {0};
    scfg.device      = r->device;
    scfg.format      = r->surface_format;
    scfg.usage       = WGPUTextureUsage_RenderAttachment;
    scfg.width       = r->width;
    scfg.height      = r->height;
    scfg.presentMode = WGPUPresentMode_Fifo;
    scfg.alphaMode   = WGPUCompositeAlphaMode_Auto;
    wgpuSurfaceConfigure(r->surface, &scfg);

    engine->renderer = r;
    if (!en_render_2d_init(engine)) {
        en_error("2D init failed");
        return false;
    }
    en_log("Render initialized: %ux%u, format=BGRA8Unorm", r->width, r->height);
    return true;
}

void en_render_shutdown(EnEngine* engine) {
    if (!engine || !engine->renderer) return;
    EnRenderer* r = engine->renderer;

    en_render_2d_shutdown(engine);

    if (r->queue)    wgpuQueueRelease(r->queue);
    if (r->device)   wgpuDeviceRelease(r->device);
    if (r->adapter)  wgpuAdapterRelease(r->adapter);
    if (r->surface)  wgpuSurfaceRelease(r->surface);
    if (r->instance) wgpuInstanceRelease(r->instance);

    free(r);
    engine->renderer = NULL;
    en_log("Render shut down");
}

// ============================================================================
// Per-frame
// ============================================================================

void en_render_frame(EnEngine* engine) {
    if (!engine || !engine->renderer) return;
    EnRenderer* r = engine->renderer;

    WGPUSurfaceTexture st = {0};
    wgpuSurfaceGetCurrentTexture(r->surface, &st);

    if (st.status != WGPUSurfaceGetCurrentTextureStatus_SuccessOptimal &&
        st.status != WGPUSurfaceGetCurrentTextureStatus_SuccessSuboptimal) {
        // Timeout, outdated, lost — try to reconfigure next frame
        if (st.status == WGPUSurfaceGetCurrentTextureStatus_Outdated ||
            st.status == WGPUSurfaceGetCurrentTextureStatus_Lost) {
            en_window_size(engine->window, &r->width, &r->height);
            if (r->width && r->height) en_render_resize(engine, r->width, r->height);
        }
        return;
    }

    WGPUTextureView view = wgpuTextureCreateView(st.texture, NULL);

    WGPURenderPassColorAttachment color = {0};
    color.view       = view;
    color.loadOp     = WGPULoadOp_Clear;
    color.storeOp    = WGPUStoreOp_Store;
    color.clearValue = r->clear_color;
    color.depthSlice = WGPU_DEPTH_SLICE_UNDEFINED;

    WGPURenderPassDescriptor rp = {0};
    rp.colorAttachmentCount = 1;
    rp.colorAttachments     = &color;

    WGPUCommandEncoder enc = wgpuDeviceCreateCommandEncoder(r->device, NULL);
    WGPURenderPassEncoder pass = wgpuCommandEncoderBeginRenderPass(enc, &rp);

    en_render_2d_render_world(engine, pass);

    wgpuRenderPassEncoderEnd(pass);
    wgpuRenderPassEncoderRelease(pass);

    WGPUCommandBuffer cmd = wgpuCommandEncoderFinish(enc, NULL);
    wgpuCommandEncoderRelease(enc);

    wgpuQueueSubmit(r->queue, 1, &cmd);
    wgpuCommandBufferRelease(cmd);
    wgpuTextureViewRelease(view);
    wgpuSurfacePresent(r->surface);
}

void en_render_resize(EnEngine* engine, u32 w, u32 h) {
    if (!engine || !engine->renderer || !w || !h) return;
    EnRenderer* r = engine->renderer;
    r->width = w;
    r->height = h;

    WGPUSurfaceConfiguration scfg = {0};
    scfg.device      = r->device;
    scfg.format      = r->surface_format;
    scfg.usage       = WGPUTextureUsage_RenderAttachment;
    scfg.width       = w;
    scfg.height      = h;
    scfg.presentMode = WGPUPresentMode_Fifo;
    scfg.alphaMode   = WGPUCompositeAlphaMode_Auto;
    wgpuSurfaceConfigure(r->surface, &scfg);

    en_log("Render resized to %ux%u", w, h);
}

// ============================================================================
// Public API
// ============================================================================

void en_render_clear_color(EnEngine* engine, f32 r_, f32 g, f32 b, f32 a) {
    if (!engine || !engine->renderer) return;
    EnRenderer* r = engine->renderer;
    r->clear_color = (WGPUColor){ r_, g, b, a };
}
