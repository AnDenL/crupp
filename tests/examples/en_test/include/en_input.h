#ifndef EN_INPUT_H
#define EN_INPUT_H

#include "types.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct EnEngine EnEngine;

// Keys — subset that we actually care about. Add more as needed.
typedef enum {
    EN_KEY_UNKNOWN = 0,
    EN_KEY_A, EN_KEY_B, EN_KEY_C, EN_KEY_D, EN_KEY_E, EN_KEY_F, EN_KEY_G,
    EN_KEY_H, EN_KEY_I, EN_KEY_J, EN_KEY_K, EN_KEY_L, EN_KEY_M, EN_KEY_N,
    EN_KEY_O, EN_KEY_P, EN_KEY_Q, EN_KEY_R, EN_KEY_S, EN_KEY_T, EN_KEY_U,
    EN_KEY_V, EN_KEY_W, EN_KEY_X, EN_KEY_Y, EN_KEY_Z,
    EN_KEY_0, EN_KEY_1, EN_KEY_2, EN_KEY_3, EN_KEY_4,
    EN_KEY_5, EN_KEY_6, EN_KEY_7, EN_KEY_8, EN_KEY_9,
    EN_KEY_SPACE, EN_KEY_ENTER, EN_KEY_ESCAPE, EN_KEY_TAB, EN_KEY_BACKSPACE,
    EN_KEY_LEFT, EN_KEY_RIGHT, EN_KEY_UP, EN_KEY_DOWN,
    EN_KEY_LSHIFT, EN_KEY_LCTRL, EN_KEY_LALT,
    EN_KEY_F1, EN_KEY_F2, EN_KEY_F3, EN_KEY_F4,
    EN_KEY_F5, EN_KEY_F6, EN_KEY_F7, EN_KEY_F8,
    EN_KEY_F9, EN_KEY_F10, EN_KEY_F11, EN_KEY_F12,
    EN_KEY_MAX
} EnKey;

typedef enum {
    EN_MOUSE_LEFT = 0,
    EN_MOUSE_RIGHT,
    EN_MOUSE_MIDDLE,
    EN_MOUSE_MAX
} EnMouseButton;

// --- Keyboard ---
bool en_input_key_down    (const EnEngine* e, EnKey k);  // held
bool en_input_key_pressed (const EnEngine* e, EnKey k);  // went down this frame
bool en_input_key_released(const EnEngine* e, EnKey k);  // went up this frame

// --- Mouse ---
bool en_input_mouse_down    (const EnEngine* e, EnMouseButton b);
bool en_input_mouse_pressed (const EnEngine* e, EnMouseButton b);
bool en_input_mouse_released(const EnEngine* e, EnMouseButton b);
void en_input_mouse_pos     (const EnEngine* e, f32* x, f32* y);   // window pixels
void en_input_mouse_delta   (const EnEngine* e, f32* dx, f32* dy); // this frame

#ifdef __cplusplus
}
#endif

#endif // EN_INPUT_H
