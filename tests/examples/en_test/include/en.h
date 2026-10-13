#ifndef EN_H
#define EN_H

#include "types.h"

#ifdef __cplusplus
extern "C" {
#endif

// ============================================================================
// Forward declarations
// ============================================================================

typedef struct EnEngine EnEngine;

// ============================================================================
// Project config (bootstrap) — read-only, lives as long as engine
// ============================================================================

typedef struct EnWindowConfig {
    const char* title;
    uint32_t    width;
    uint32_t    height;
    bool        fullscreen;
    bool        resizable;
    bool        transparent;
    bool        vsync;
} EnWindowConfig;

typedef struct EnProjectConfig {
    const char*    name;
    const char*    version;
    const char*    start_scene;
    const char*    assets_path;
    const char*    scripts_path;
    EnWindowConfig window;
} EnProjectConfig;

// ============================================================================
// Plugins
// ============================================================================

typedef struct EnPlugin {
    const char* name;
    void (*on_init)(EnEngine* engine, void* user_data);       // at register
    void (*on_start)(EnEngine* engine, void* user_data);      // after render init
    void (*on_update)(EnEngine* engine, void* user_data);     // per-frame
    void (*on_shutdown)(EnEngine* engine, void* user_data);   // at destroy
    void* user_data;
} EnPlugin;

// ============================================================================
// Assets
// ============================================================================

typedef uint32_t EnAssetId;
#define EN_ASSET_INVALID ((EnAssetId)0)

EnAssetId   en_asset_load   (EnEngine* engine, const char* path);
void*       en_asset_data   (const EnEngine* engine, EnAssetId id);
size_t      en_asset_size   (const EnEngine* engine, EnAssetId id);
const char* en_asset_path   (const EnEngine* engine, EnAssetId id);
void        en_asset_release(EnEngine* engine, EnAssetId id);

// ============================================================================
// Asset handlers
// ============================================================================

typedef struct EnAssetHandler {
    const char* extension;
    void* (*load)(EnEngine* engine, const char* path,
                  const u8* bytes, usize size, void* user_data);
    void  (*free)(EnEngine* engine, void* asset, void* user_data);
    void* user_data;
} EnAssetHandler;

// ============================================================================
// Lifecycle
// ============================================================================

EnEngine* en_create(void);
void      en_destroy(EnEngine* engine);

bool en_load_project(EnEngine* engine, const char* path);
const EnProjectConfig* en_project_config(const EnEngine* engine);

bool en_register_plugin(EnEngine* engine, const EnPlugin* plugin);
bool en_register_asset_handler(EnEngine* engine, const EnAssetHandler* handler);

int  en_run(EnEngine* engine);
void en_stop(EnEngine* engine);

// ============================================================================
// Logging
// ============================================================================

void en_log  (const char* fmt, ...);
void en_warn (const char* fmt, ...);
void en_error(const char* fmt, ...);

// ============================================================================
// Engine-owned subsystems (created automatically by en_create)
// ============================================================================

typedef struct EnWorld EnWorld;
typedef struct EnScheduler EnScheduler;

EnWorld*     en_world(EnEngine* engine);
EnScheduler* en_scheduler(EnEngine* engine);

u32 en_comp_transform2d(const EnEngine* engine);
u32 en_comp_sprite2d(const EnEngine* engine);
u32 en_comp_camera2d(const EnEngine* engine);

f32 en_delta_time  (const EnEngine* engine);
f64 en_elapsed_time(const EnEngine* engine);
u64 en_frame_count (const EnEngine* engine);

#include "en_scene.h"

#ifdef __cplusplus
}
#endif

#endif // EN_H
