#ifndef EN_INPUT_INTERNAL_H
#define EN_INPUT_INTERNAL_H

#include "en_input.h"
#include "types.h"

typedef struct {
    bool down[EN_KEY_MAX];
    bool pressed[EN_KEY_MAX];
    bool released[EN_KEY_MAX];
} EnKeyState;

typedef struct {
    bool down[EN_MOUSE_MAX];
    bool pressed[EN_MOUSE_MAX];
    bool released[EN_MOUSE_MAX];
} EnMouseState;

typedef struct {
    EnKeyState   keys;
    EnMouseState mouse;
    f32 mouse_x, mouse_y;
    f32 mouse_dx, mouse_dy;
    f32 scroll_x, scroll_y;
    f32 last_mouse_x, last_mouse_y;
    bool first_frame;
} EnInput;

void en_input_poll(EnEngine* engine);

#endif
