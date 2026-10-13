#ifndef EN_WINDOW_INTERNAL_H
#define EN_WINDOW_INTERNAL_H

#include "../internal.h"

typedef enum {
    EN_WINDOW_PLATFORM_UNKNOWN,
    EN_WINDOW_PLATFORM_X11,
    EN_WINDOW_PLATFORM_WAYLAND,
} EnWindowPlatform;

bool  en_window_init(void);
void  en_window_shutdown(void);
void* en_window_create(const EnWindowConfig* cfg);
void  en_window_destroy(void* window);
bool  en_window_should_close(void* window);
void  en_window_poll(void* window);
void  en_window_swap(void* window);

// Platform info for wgpu
EnWindowPlatform en_window_platform(void* window);
void* en_window_wayland_display(void* window);
void* en_window_wayland_surface(void* window);
void* en_window_x11_display(void* window);
u64   en_window_x11_window(void* window);

// Current size (window may have been resized)
void en_window_size(void* window, u32* out_w, u32* out_h);

// Input polling (raw device state)
bool en_window_key_down  (void* window, EnKey k);
bool en_window_mouse_down(void* window, EnMouseButton b);
void en_window_mouse_pos (void* window, f32* x, f32* y);

#endif // EN_WINDOW_INTERNAL_H
