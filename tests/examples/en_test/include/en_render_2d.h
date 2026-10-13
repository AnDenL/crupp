#ifndef EN_RENDER_2D_H
#define EN_RENDER_2D_H

#include "en.h"
#include "en_components.h"
#include "types.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct EnTexture EnTexture;

// One sprite instance.
typedef struct {
    f32 x, y;             // center, world units
    f32 w, h;             // size, world units
    f32 rotation;         // radians, CCW
    f32 r, g, b, a;       // tint
} Instance2D;

// --- Texture ---
EnTexture* en_render_2d_texture_load(EnEngine* engine, const char* path);
EnTexture* en_render_2d_texture_white(EnEngine* engine);
void       en_render_2d_texture_free (EnEngine* engine, EnTexture* tex);
u32        en_render_2d_texture_width (const EnTexture* tex);
u32        en_render_2d_texture_height(const EnTexture* tex);

// --- Camera ---
void en_render_2d_set_camera(EnEngine* engine, EnCamera2D cam);

// --- Drawing ---
void en_render_2d_begin (EnEngine* engine);
void en_render_2d_draw  (EnEngine* engine, const EnTexture* tex, Instance2D s);
void en_render_2d_end   (EnEngine* engine);

#ifdef __cplusplus
}
#endif

#endif // EN_RENDER_2D_H
