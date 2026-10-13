#ifndef EN_INTERNAL_H
#define EN_INTERNAL_H

#include "en.h"
#include "en_ecs.h"
#include "render/internal.h"
#include "en_scene.h"
#include "input/internal.h"    // ← додано
#include "types.h"

#define EN_MAX_PLUGINS        64
#define EN_MAX_ASSET_HANDLERS 32
#define EN_MAX_ASSETS         4096

typedef struct { EnPlugin plugin; bool active; } EnPluginSlot;
typedef struct { EnAssetHandler handler; bool active; } EnAssetHandlerSlot;

typedef struct {
    char*  path;
    u64    hash;
    const EnAssetHandler* handler;
    void*  data;
    usize  size;
    u32    refcount;
    bool   active;
} EnAssetSlot;

struct EnEngine {
    bool running;
    bool should_stop;

    EnProjectConfig config;
    bool            config_loaded;

    void* window;
    EnRenderer* renderer;

    EnWorld*     world;
    EnScheduler* scheduler;
    u32          comp_transform2d;
    u32          comp_sprite2d;
    u32          comp_camera2d;

    EnInput      input;

    double   delta_time;
    double   elapsed_time;
    u64      frame_count;

    EnPluginSlot       plugins[EN_MAX_PLUGINS];
    usize              plugin_count;

    EnAssetHandlerSlot asset_handlers[EN_MAX_ASSET_HANDLERS];
    usize              asset_handler_count;

    EnAssetSlot        assets[EN_MAX_ASSETS];
};

void en_log(const char* fmt, ...);
void en_warn(const char* fmt, ...);
void en_error(const char* fmt, ...);

u64 en_hash_fnv1a(const char* str);

bool en_config_load(const char* path, EnProjectConfig* out);
void en_config_free(EnProjectConfig* cfg);

const EnAssetHandler* en_find_asset_handler(EnEngine* engine, const char* ext);

#endif
