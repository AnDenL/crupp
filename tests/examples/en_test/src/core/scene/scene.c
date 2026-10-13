#include "internal.h"

#include "toml.h"

#include <stdio.h>
#include <stdlib.h>

// ============================================================================
// Component parsers
// ============================================================================

static void parse_transform2d(toml_table_t* t, EnTransform2D* out) {
    *out = (EnTransform2D){ .scale_x = 1.0f, .scale_y = 1.0f };
    toml_datum_t d;

    d = toml_double_in(t, "x");        if (d.ok) out->x        = (f32)d.u.d;
    d = toml_double_in(t, "y");        if (d.ok) out->y        = (f32)d.u.d;
    d = toml_double_in(t, "rotation"); if (d.ok) out->rotation = (f32)d.u.d;
    d = toml_double_in(t, "scale_x");  if (d.ok) out->scale_x  = (f32)d.u.d;
    d = toml_double_in(t, "scale_y");  if (d.ok) out->scale_y  = (f32)d.u.d;
}

static void parse_sprite2d(EnEngine* engine, toml_table_t* t, EnSprite2D* out) {
    *out = (EnSprite2D){
        .texture = EN_ASSET_INVALID,
        .w = 1.0f, .h = 1.0f,
        .r = 1.0f, .g = 1.0f, .b = 1.0f, .a = 1.0f,
        .z = 0,
    };
    toml_datum_t d;

    d = toml_string_in(t, "texture");
    if (d.ok) {
        out->texture = en_asset_load(engine, d.u.s);
        if (out->texture == EN_ASSET_INVALID)
            en_warn("Scene: texture '%s' could not be loaded", d.u.s);
        free(d.u.s);
    }

    d = toml_double_in(t, "width");  if (d.ok) out->w  = (f32)d.u.d;
    d = toml_double_in(t, "height"); if (d.ok) out->h = (f32)d.u.d;
    d = toml_double_in(t, "r");      if (d.ok) out->r      = (f32)d.u.d;
    d = toml_double_in(t, "g");      if (d.ok) out->g      = (f32)d.u.d;
    d = toml_double_in(t, "b");      if (d.ok) out->b      = (f32)d.u.d;
    d = toml_double_in(t, "a");      if (d.ok) out->a      = (f32)d.u.d;
    d = toml_int_in(t, "z");         if (d.ok) out->z      = (i32)d.u.i;
}

static void parse_camera2d(toml_table_t* t, EnCamera2D* out) {
    *out = (EnCamera2D){ .x = 0, .y = 0, .zoom = 64.0f };
    toml_datum_t d;
    d = toml_double_in(t, "x");    if (d.ok) out->x    = (f32)d.u.d;
    d = toml_double_in(t, "y");    if (d.ok) out->y    = (f32)d.u.d;
    d = toml_double_in(t, "zoom"); if (d.ok) out->zoom = (f32)d.u.d;
}

// ============================================================================
// Public
// ============================================================================

bool en_scene_load_and_spawn(EnEngine* engine, const char* path) {
    if (!engine || !path) return false;

    FILE* fp = fopen(path, "r");
    if (!fp) {
        en_warn("Scene not found: %s", path);
        return false;
    }

    char errbuf[256];
    toml_table_t* root = toml_parse_file(fp, errbuf, sizeof(errbuf));
    fclose(fp);

    if (!root) {
        en_error("Scene parse error in %s: %s", path, errbuf);
        return false;
    }

    toml_array_t* entities = toml_array_in(root, "entity");
    if (!entities) {
        en_warn("Scene %s has no [[entity]] arrays", path);
        toml_free(root);
        return true;
    }

    int n = toml_array_nelem(entities);
    usize spawned = 0;

    for (int i = 0; i < n; i++) {
        toml_table_t* e = toml_table_at(entities, i);
        if (!e) continue;

        EnBundleEntry bundle[3];
        usize         count = 0;

        EnTransform2D tr;
        EnSprite2D    sp;

        EnCamera2D cam;

        toml_table_t* tr_t = toml_table_in(e, "transform");
        if (tr_t) {
            parse_transform2d(tr_t, &tr);
            bundle[count].comp_id = engine->comp_transform2d;
            bundle[count].data    = &tr;
            count++;
        }

        toml_table_t* sp_t = toml_table_in(e, "sprite");
        if (sp_t) {
            parse_sprite2d(engine, sp_t, &sp);
            bundle[count].comp_id = engine->comp_sprite2d;
            bundle[count].data    = &sp;
            count++;
        }

        toml_table_t* cam_t = toml_table_in(e, "camera");
        if (cam_t) {
            parse_camera2d(cam_t, &cam);
            bundle[count].comp_id = engine->comp_camera2d;
            bundle[count].data    = &cam;
            count++;
        }

        if (count == 0) continue;

        en_world_spawn_bundle_raw(engine->world, bundle, count);
        spawned++;
    }

    toml_free(root);
    en_log("Scene loaded: %s (%zu entities)", path, spawned);
    return true;
}
