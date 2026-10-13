#ifndef EN_COMPONENTS_H
#define EN_COMPONENTS_H

#include "en.h"

#ifdef __cplusplus
extern "C" {
#endif

// Camera2D — render viewport. First camera entity in the world wins.
typedef struct {
    f32 x, y;
    f32 zoom;      // pixels per world unit
} EnCamera2D;

// Transform2D — position, rotation, scale in world units.
typedef struct {
    f32 x, y;
    f32 rotation;        // radians, CCW
    f32 scale_x, scale_y;
} EnTransform2D;

// Sprite2D — visual, references an asset by id.
typedef struct {
    EnAssetId texture;   // EN_ASSET_INVALID = 1x1 white
    f32       w, h;
    f32       rotation;
    f32       r, g, b, a;
    i32       z;         // draw order (unused for now, batching by texture)
} EnSprite2D;

#ifdef __cplusplus
}
#endif

#endif // EN_COMPONENTS_H
