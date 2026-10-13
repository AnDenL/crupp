#include "window/internal.h"

#include <string.h>

bool en_input_key_down(const EnEngine* e, EnKey k) { return e && k < EN_KEY_MAX && e->input.keys.down[k]; }
bool en_input_key_pressed (const EnEngine* e, EnKey k) { return e && k < EN_KEY_MAX && e->input.keys.pressed[k]; }
bool en_input_key_released(const EnEngine* e, EnKey k) { return e && k < EN_KEY_MAX && e->input.keys.released[k]; }

bool en_input_mouse_down    (const EnEngine* e, EnMouseButton b) { return e && b < EN_MOUSE_MAX && e->input.mouse.down[b]; }
bool en_input_mouse_pressed (const EnEngine* e, EnMouseButton b) { return e && b < EN_MOUSE_MAX && e->input.mouse.pressed[b]; }
bool en_input_mouse_released(const EnEngine* e, EnMouseButton b) { return e && b < EN_MOUSE_MAX && e->input.mouse.released[b]; }

void en_input_mouse_pos(const EnEngine* e, f32* x, f32* y) {
    if (!e) { *x = *y = 0; return; }
    *x = e->input.mouse_x;
    *y = e->input.mouse_y;
}

void en_input_mouse_delta(const EnEngine* e, f32* dx, f32* dy) {
    if (!e) { *dx = *dy = 0; return; }
    *dx = e->input.mouse_dx;
    *dy = e->input.mouse_dy;
}

void en_input_poll(EnEngine* engine) {
    EnInput* in = &engine->input;

    memset(in->keys.pressed,  0, sizeof(in->keys.pressed));
    memset(in->keys.released, 0, sizeof(in->keys.released));
    memset(in->mouse.pressed,  0, sizeof(in->mouse.pressed));
    memset(in->mouse.released, 0, sizeof(in->mouse.released));

    f32 mx, my;
    en_window_mouse_pos(engine->window, &mx, &my);

    if (!in->first_frame) {
        in->mouse_dx = mx - in->last_mouse_x;
        in->mouse_dy = my - in->last_mouse_y;
    } else {
        in->mouse_dx = 0;
        in->mouse_dy = 0;
        in->first_frame = false;
    }
    in->mouse_x = mx;
    in->mouse_y = my;
    in->last_mouse_x = mx;
    in->last_mouse_y = my;

    for (int k = 1; k < EN_KEY_MAX; k++) {
        bool now  = en_window_key_down(engine->window, (EnKey)k);
        bool prev = in->keys.down[k];
        if (now && !prev) in->keys.pressed[k] = true;
        if (!now && prev) in->keys.released[k] = true;
        in->keys.down[k] = now;
    }

    for (int b = 0; b < EN_MOUSE_MAX; b++) {
        bool now  = en_window_mouse_down(engine->window, (EnMouseButton)b);
        bool prev = in->mouse.down[b];
        if (now && !prev) in->mouse.pressed[b] = true;
        if (!now && prev) in->mouse.released[b] = true;
        in->mouse.down[b] = now;
    }
}
