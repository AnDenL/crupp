#ifndef EN_SCENE_H
#define EN_SCENE_H

#ifdef __cplusplus
extern "C" {
#endif

typedef struct EnEngine EnEngine;

// Load a .scene.toml and spawn all [[entity]] entries into engine's world.
// Missing file → warn + return false. Parse error → error + return false.
bool en_scene_load_and_spawn(EnEngine* engine, const char* path);

#ifdef __cplusplus
}
#endif

#endif // EN_SCENE_H
