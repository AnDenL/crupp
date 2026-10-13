#ifndef EN_RENDER_H
#define EN_RENDER_H

#include "types.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct EnEngine EnEngine;

// Clear color (default: dark blue-gray)
void en_render_clear_color(EnEngine* engine, f32 r, f32 g, f32 b, f32 a);

#ifdef __cplusplus
}
#endif

#endif // EN_RENDER_H
