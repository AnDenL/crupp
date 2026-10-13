#include "internal.h"

#define GLFW_INCLUDE_NONE
#define GLFW_EXPOSE_NATIVE_WAYLAND
#define GLFW_EXPOSE_NATIVE_X11
#include <GLFW/glfw3.h>
#include <GLFW/glfw3native.h>

#include <stdlib.h>
#include <string.h>

typedef struct EnWindow {
    GLFWwindow* handle;
    bool        vsync;
} EnWindow;

static bool g_glfw_initialized = false;

static void en_glfw_error_callback(int code, const char* desc) {
    en_error("GLFW error %d: %s", code, desc);
}

bool en_window_init(void) {
    if (g_glfw_initialized) return true;
    glfwSetErrorCallback(en_glfw_error_callback);
    if (!glfwInit()) {
        en_error("Failed to initialize GLFW");
        return false;
    }
    g_glfw_initialized = true;
    return true;
}

void en_window_shutdown(void) {
    if (!g_glfw_initialized) return;
    glfwTerminate();
    g_glfw_initialized = false;
}

void* en_window_create(const EnWindowConfig* cfg) {
    if (!cfg) return NULL;
    if (!en_window_init()) return NULL;

    glfwWindowHint(GLFW_RESIZABLE,               cfg->resizable ? GLFW_TRUE : GLFW_FALSE);
    glfwWindowHint(GLFW_TRANSPARENT_FRAMEBUFFER, cfg->transparent ? GLFW_TRUE : GLFW_FALSE);
    glfwWindowHint(GLFW_CLIENT_API,              GLFW_NO_API);

    GLFWmonitor* monitor = cfg->fullscreen ? glfwGetPrimaryMonitor() : NULL;
    GLFWwindow* handle = glfwCreateWindow(
        (int)cfg->width, (int)cfg->height,
        cfg->title ? cfg->title : "En Engine",
        monitor, NULL);

    if (!handle) {
        en_error("Failed to create GLFW window");
        return NULL;
    }

    EnWindow* w = calloc(1, sizeof(EnWindow));
    w->handle = handle;
    w->vsync  = cfg->vsync;

    en_log("Window created: %ux%u, title=\"%s\"",
           cfg->width, cfg->height, cfg->title ? cfg->title : "(null)");

    return w;
}

void en_window_destroy(void* window) {
    if (!window) return;
    EnWindow* w = window;
    if (w->handle) glfwDestroyWindow(w->handle);
    free(w);
}

bool en_window_should_close(void* window) {
    if (!window) return true;
    EnWindow* w = window;
    return glfwWindowShouldClose(w->handle);
}

void en_window_poll(void* window) {
    (void)window;
    glfwPollEvents();
}

void en_window_swap(void* window) {
    (void)window;
    // No-op. Presentation is handled by wgpu surface present.
}

void en_window_size(void* window, u32* out_w, u32* out_h) {
    if (!window) { *out_w = 0; *out_h = 0; return; }
    EnWindow* w = window;
    int fw, fh;
    glfwGetFramebufferSize(w->handle, &fw, &fh);
    *out_w = (u32)fw;
    *out_h = (u32)fh;
}

// ============================================================================
// Platform info for wgpu
// ============================================================================

EnWindowPlatform en_window_platform(void* window) {
    if (!window) return EN_WINDOW_PLATFORM_UNKNOWN;
    EnWindow* w = window;

    int platform = glfwGetPlatform();
    if (platform == GLFW_PLATFORM_WAYLAND) return EN_WINDOW_PLATFORM_WAYLAND;
    if (platform == GLFW_PLATFORM_X11)     return EN_WINDOW_PLATFORM_X11;
    (void)w;
    return EN_WINDOW_PLATFORM_UNKNOWN;
}

void* en_window_wayland_display(void* window) {
    if (!window) return NULL;
    EnWindow* w = window;
    return (void*)glfwGetWaylandDisplay();
}

void* en_window_wayland_surface(void* window) {
    if (!window) return NULL;
    EnWindow* w = window;
    return (void*)glfwGetWaylandWindow(w->handle);
}

void* en_window_x11_display(void* window) {
    if (!window) return NULL;
    EnWindow* w = window;
    return (void*)glfwGetX11Display();
}

u64 en_window_x11_window(void* window) {
    if (!window) return 0;
    EnWindow* w = window;
    return (u64)glfwGetX11Window(w->handle);
}

static int glfw_key_from_en(EnKey k) {
    if (k >= EN_KEY_A && k <= EN_KEY_Z) return 'A' + (k - EN_KEY_A);
    if (k >= EN_KEY_0 && k <= EN_KEY_9) return '0' + (k - EN_KEY_0);
    switch (k) {
        case EN_KEY_SPACE:     return 32;
        case EN_KEY_ENTER:     return 257;
        case EN_KEY_ESCAPE:    return 256;
        case EN_KEY_TAB:       return 258;
        case EN_KEY_BACKSPACE: return 259;
        case EN_KEY_LEFT:      return 263;
        case EN_KEY_RIGHT:     return 262;
        case EN_KEY_UP:        return 265;
        case EN_KEY_DOWN:      return 264;
        case EN_KEY_LSHIFT:    return 340;
        case EN_KEY_LCTRL:     return 341;
        case EN_KEY_LALT:      return 342;
        case EN_KEY_F1:  return 290;
        case EN_KEY_F2:  return 291;
        case EN_KEY_F3:  return 292;
        case EN_KEY_F4:  return 293;
        case EN_KEY_F5:  return 294;
        case EN_KEY_F6:  return 295;
        case EN_KEY_F7:  return 296;
        case EN_KEY_F8:  return 297;
        case EN_KEY_F9:  return 298;
        case EN_KEY_F10: return 299;
        case EN_KEY_F11: return 300;
        case EN_KEY_F12: return 301;
        default: return -1;
    }
}

bool en_window_key_down(void* window, EnKey k) {
    if (!window || k == EN_KEY_UNKNOWN) return false;
    EnWindow* w = window;
    int gk = glfw_key_from_en(k);
    if (gk < 0) return false;
    return glfwGetKey(w->handle, gk) == GLFW_PRESS;
}

bool en_window_mouse_down(void* window, EnMouseButton b) {
    if (!window) return false;
    EnWindow* w = window;
    return glfwGetMouseButton(w->handle, (int)b) == GLFW_PRESS;
}

void en_window_mouse_pos(void* window, f32* x, f32* y) {
    if (!window) { *x = *y = 0; return; }
    EnWindow* w = window;
    double mx, my;
    glfwGetCursorPos(w->handle, &mx, &my);
    *x = (f32)mx;
    *y = (f32)my;
}
