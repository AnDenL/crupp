#include "internal.h"

#include "toml.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static char* en_strdup_or(const char* s, const char* fallback) {
    const char* src = s ? s : fallback;
    if (!src) return NULL;
    size_t n = strlen(src);
    char* out = malloc(n + 1);
    if (out) memcpy(out, src, n + 1);
    return out;
}

bool en_config_load(const char* path, EnProjectConfig* out) {
    if (!path || !out) return false;

    FILE* fp = fopen(path, "r");
    if (!fp) {
        en_error("Cannot open project file: %s", path);
        return false;
    }

    char errbuf[256];
    toml_table_t* root = toml_parse_file(fp, errbuf, sizeof(errbuf));
    fclose(fp);

    if (!root) {
        en_error("Failed to parse %s: %s", path, errbuf);
        return false;
    }

    memset(out, 0, sizeof(*out));

    // ---- [project] ----
    toml_table_t* project = toml_table_in(root, "project");
    if (project) {
        toml_datum_t name    = toml_string_in(project, "name");
        toml_datum_t version = toml_string_in(project, "version");
        toml_datum_t scene   = toml_string_in(project, "start_scene");

        out->name        = en_strdup_or(name.ok    ? name.u.s    : NULL, "unnamed");
        out->version     = en_strdup_or(version.ok ? version.u.s : NULL, "0.0.0");
        out->start_scene = scene.ok ? en_strdup_or(scene.u.s, NULL) : NULL;

        if (name.ok)    free(name.u.s);
        if (version.ok) free(version.u.s);
        if (scene.ok)   free(scene.u.s);
    } else {
        out->name    = en_strdup_or(NULL, "unnamed");
        out->version = en_strdup_or(NULL, "0.0.0");
    }

    // ---- [window] ----
    out->window.width       = 1280;
    out->window.height      = 720;
    out->window.title       = en_strdup_or(NULL, "En Engine");
    out->window.fullscreen  = false;
    out->window.resizable   = true;
    out->window.transparent = false;
    out->window.vsync       = true;

    toml_table_t* window = toml_table_in(root, "window");
    if (window) {
        toml_datum_t title       = toml_string_in(window, "title");
        toml_datum_t width       = toml_int_in(window, "width");
        toml_datum_t height      = toml_int_in(window, "height");
        toml_datum_t fullscreen  = toml_bool_in(window, "fullscreen");
        toml_datum_t resizable   = toml_bool_in(window, "resizable");
        toml_datum_t transparent = toml_bool_in(window, "transparent");
        toml_datum_t vsync       = toml_bool_in(window, "vsync");

        if (title.ok)       { free((void*)out->window.title); out->window.title = en_strdup_or(title.u.s, "En Engine"); free(title.u.s); }
        if (width.ok)       out->window.width       = (uint32_t)width.u.i;
        if (height.ok)      out->window.height      = (uint32_t)height.u.i;
        if (fullscreen.ok)  out->window.fullscreen  = fullscreen.u.b;
        if (resizable.ok)   out->window.resizable   = resizable.u.b;
        if (transparent.ok) out->window.transparent = transparent.u.b;
        if (vsync.ok)       out->window.vsync       = vsync.u.b;
    }

    // ---- [paths] ----
    toml_table_t* paths = toml_table_in(root, "paths");
    if (paths) {
        toml_datum_t assets  = toml_string_in(paths, "assets");
        toml_datum_t scripts = toml_string_in(paths, "scripts");
        out->assets_path  = en_strdup_or(assets.ok  ? assets.u.s  : NULL, "assets");
        out->scripts_path = en_strdup_or(scripts.ok ? scripts.u.s : NULL, "assets/scripts");
        if (assets.ok)  free(assets.u.s);
        if (scripts.ok) free(scripts.u.s);
    } else {
        out->assets_path  = en_strdup_or(NULL, "assets");
        out->scripts_path = en_strdup_or(NULL, "assets/scripts");
    }

    toml_free(root);
    return true;
}

void en_config_free(EnProjectConfig* cfg) {
    if (!cfg) return;
    free((void*)cfg->name);
    free((void*)cfg->version);
    free((void*)cfg->start_scene);
    free((void*)cfg->assets_path);
    free((void*)cfg->scripts_path);
    free((void*)cfg->window.title);
    memset(cfg, 0, sizeof(*cfg));
}
