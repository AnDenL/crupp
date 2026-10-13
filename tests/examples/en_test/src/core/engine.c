#include "internal.h"
#include "window/internal.h"
#include "scene/internal.h"
#include "input/internal.h"

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

// ---------------- logging ----------------

static void en_vlog(const char* level, const char* fmt, va_list args) {
    fprintf(stderr, "[en/%s] ", level);
    vfprintf(stderr, fmt, args);
    fprintf(stderr, "\n");
}

void en_log  (const char* fmt, ...) { va_list a; va_start(a, fmt); en_vlog("info",  fmt, a); va_end(a); }
void en_warn (const char* fmt, ...) { va_list a; va_start(a, fmt); en_vlog("warn",  fmt, a); va_end(a); }
void en_error(const char* fmt, ...) { va_list a; va_start(a, fmt); en_vlog("error", fmt, a); va_end(a); }

// ---------------- lifecycle ----------------

EnEngine* en_create(void) {
    EnEngine* e = calloc(1, sizeof(EnEngine));
    if (!e) { en_error("Failed to allocate engine"); return NULL; }

    // Engine owns the world and scheduler.
    e->world = en_world_create();
    if (!e->world) { free(e); return NULL; }

    e->scheduler = en_scheduler_create();
    if (!e->scheduler) { en_world_destroy(e->world); free(e); return NULL; }

    // Register standard components with stable hashes derived from names.
    e->comp_transform2d = en_component_register(
        e->world, en_hash_fnv1a("EnTransform2D"),
        sizeof(EnTransform2D), _Alignof(EnTransform2D), NULL);

    e->comp_sprite2d = en_component_register(
        e->world, en_hash_fnv1a("EnSprite2D"),
        sizeof(EnSprite2D), _Alignof(EnSprite2D), NULL);

    e->comp_camera2d = en_component_register(
        e->world, en_hash_fnv1a("EnCamera2D"),
        sizeof(EnCamera2D), _Alignof(EnCamera2D), NULL);

    en_log("Engine created (world=%p, scheduler=%p)",
           (void*)e->world, (void*)e->scheduler);
    return e;
}

void en_destroy(EnEngine* engine) {
    if (!engine) return;

    // 1. Plugin shutdown hooks
    for (size_t i = engine->plugin_count; i > 0; i--) {
        EnPluginSlot* s = &engine->plugins[i - 1];
        if (s->active && s->plugin.on_shutdown)
            s->plugin.on_shutdown(engine, s->plugin.user_data);
    }

    // 2. Free assets BEFORE renderer — texture handles must release
    //    while wgpu device is still alive.
    for (size_t i = 0; i < EN_MAX_ASSETS; i++) {
        EnAssetSlot* s = &engine->assets[i];
        if (s->active) {
            if (s->handler && s->handler->free)
                s->handler->free(engine, s->data, s->handler->user_data);
            free(s->path);
            s->active = false;
        }
    }

    // 3. Shut down renderer (releases device, surface, instance).
    if (engine->renderer) {
        en_render_shutdown(engine);
    }

    // 4. Window
    if (engine->window) {
        en_window_destroy(engine->window);
        engine->window = NULL;
    }
    en_window_shutdown();

    // 5. Config
    if (engine->config_loaded)
        en_config_free(&engine->config);

    // 6. ECS
    if (engine->scheduler) en_scheduler_destroy(engine->scheduler);
    if (engine->world)     en_world_destroy(engine->world);

    free(engine);
    en_log("Engine destroyed");
}

// ---------------- project ----------------

bool en_load_project(EnEngine* engine, const char* path) {
    if (!engine || !path) return false;
    if (engine->config_loaded) {
        en_warn("Project already loaded, ignoring %s", path);
        return false;
    }
    if (!en_config_load(path, &engine->config)) return false;
    engine->config_loaded = true;

    en_log("Project loaded: %s v%s", engine->config.name, engine->config.version);
    return true;
}

const EnProjectConfig* en_project_config(const EnEngine* engine) {
    if (!engine || !engine->config_loaded) return NULL;
    return &engine->config;
}

// ---------------- engine-owned subsystems ----------------

EnWorld*     en_world(EnEngine* engine)     { return engine ? engine->world : NULL; }
EnScheduler* en_scheduler(EnEngine* engine) { return engine ? engine->scheduler : NULL; }

u32 en_comp_transform2d(const EnEngine* e)  { return e ? e->comp_transform2d : 0; }
u32 en_comp_sprite2d(const EnEngine* e)     { return e ? e->comp_sprite2d : 0; }
u32 en_comp_camera2d(const EnEngine* e)     { return e ? e->comp_camera2d : 0; }

f32 en_delta_time  (const EnEngine* e) { return e ? (f32)e->delta_time : 0.0f; }
f64 en_elapsed_time(const EnEngine* e) { return e ? e->elapsed_time : 0.0; }
u64 en_frame_count (const EnEngine* e) { return e ? e->frame_count : 0; }

// ---------------- plugins ----------------

bool en_register_plugin(EnEngine* engine, const EnPlugin* plugin) {
    if (!engine || !plugin || !plugin->name) return false;
    if (engine->plugin_count >= EN_MAX_PLUGINS) return false;

    EnPluginSlot* s = &engine->plugins[engine->plugin_count++];
    s->plugin = *plugin;
    s->active = true;

    en_log("Registering plugin: %s", plugin->name);
    if (plugin->on_init) plugin->on_init(engine, plugin->user_data);
    return true;
}

// ---------------- assets ----------------

bool en_register_asset_handler(EnEngine* engine, const EnAssetHandler* handler) {
    if (!engine || !handler || !handler->extension) return false;
    if (engine->asset_handler_count >= EN_MAX_ASSET_HANDLERS) return false;

    EnAssetHandlerSlot* s = &engine->asset_handlers[engine->asset_handler_count++];
    s->handler = *handler;
    s->active = true;

    en_log("Registered asset handler for .%s", handler->extension);
    return true;
}

// ---------------- timing ----------------

static double en_now(void) {
    struct timespec ts;
    timespec_get(&ts, TIME_UTC);
    return (double)ts.tv_sec + (double)ts.tv_nsec * 1e-9;
}

// ---------------- main loop ----------------

int en_run(EnEngine* engine) {
    if (!engine) return 1;
    if (!engine->config_loaded) {
        en_error("No project loaded. Call en_load_project() first.");
        return 1;
    }

    engine->window = en_window_create(&engine->config.window);
    if (!engine->window) { en_error("Failed to create window"); return 1; }

    if (!en_render_init(engine)) {
        en_error("Render init failed");
        return 1;
    }

    if (engine->config.start_scene && engine->config.start_scene[0]) {
        en_scene_load_and_spawn(engine, engine->config.start_scene);
    }

    // Render init registered PNG handler etc; now fire plugin on_start.
    for (size_t p = 0; p < engine->plugin_count; p++) {
        EnPluginSlot* s = &engine->plugins[p];
        if (s->active && s->plugin.on_start)
            s->plugin.on_start(engine, s->plugin.user_data);
    }

    engine->running = true;
    engine->should_stop = false;
    en_log("Engine running");

    double last = en_now();

    while (!engine->should_stop && !en_window_should_close(engine->window)) {
        double now = en_now();
        engine->delta_time    = now - last;
        engine->elapsed_time += engine->delta_time;
        last = now;
        engine->frame_count++;

        en_window_poll(engine->window);
        en_input_poll(engine);
        en_scheduler_run(engine->scheduler, engine);

        // Plugin update hooks (optional, for things outside ECS).
        for (size_t p = 0; p < engine->plugin_count; p++) {
            EnPluginSlot* s = &engine->plugins[p];
            if (s->active && s->plugin.on_update)
                s->plugin.on_update(engine, s->plugin.user_data);
        }

        // Built-in render: queries world + draws. Engine owns this.
        en_render_frame(engine);

        en_window_swap(engine->window);
    }

    engine->running = false;
    en_log("Engine stopped after %llu frames (%.2fs)",
           (unsigned long long)engine->frame_count, engine->elapsed_time);
    return 0;
}

void en_stop(EnEngine* engine) { if (engine) engine->should_stop = true; }
