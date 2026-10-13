#include "internal.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// ============================================================================
// Hash (FNV-1a 64-bit)
// ============================================================================

uint64_t en_hash_fnv1a(const char* str) {
    uint64_t h = 0xcbf29ce484222325ULL;
    for (const unsigned char* p = (const unsigned char*)str; *p; p++) {
        h ^= (uint64_t)*p;
        h *= 0x100000001b3ULL;
    }
    return h;
}

// ============================================================================
// Extension helper
// ============================================================================

static const char* en_ext_of(const char* path) {
    const char* dot = strrchr(path, '.');
    return dot ? dot + 1 : "";
}

// ============================================================================
// Find handler by extension
// ============================================================================

const EnAssetHandler* en_find_asset_handler(EnEngine* engine, const char* ext) {
    for (size_t i = 0; i < engine->asset_handler_count; i++) {
        EnAssetHandlerSlot* s = &engine->asset_handlers[i];
        if (s->active && strcmp(s->handler.extension, ext) == 0)
            return &s->handler;
    }
    return NULL;
}

// ============================================================================
// Find existing asset by hash + path (dedup)
// ============================================================================

static EnAssetId en_asset_find(const EnEngine* engine, uint64_t hash, const char* path) {
    for (size_t i = 0; i < EN_MAX_ASSETS; i++) {
        const EnAssetSlot* s = &engine->assets[i];
        if (s->active && s->hash == hash && strcmp(s->path, path) == 0)
            return (EnAssetId)(i + 1); // +1 бо 0 = invalid
    }
    return EN_ASSET_INVALID;
}

// ============================================================================
// Find free slot
// ============================================================================

static EnAssetSlot* en_asset_free_slot(EnEngine* engine) {
    for (size_t i = 0; i < EN_MAX_ASSETS; i++) {
        if (!engine->assets[i].active)
            return &engine->assets[i];
    }
    return NULL;
}

// ============================================================================
// Read file
// ============================================================================

static uint8_t* en_read_file(const char* path, size_t* out_size) {
    FILE* fp = fopen(path, "rb");
    if (!fp) return NULL;

    fseek(fp, 0, SEEK_END);
    long size = ftell(fp);
    fseek(fp, 0, SEEK_SET);

    if (size < 0) { fclose(fp); return NULL; }

    uint8_t* buf = malloc((size_t)size);
    if (!buf) { fclose(fp); return NULL; }

    size_t read = fread(buf, 1, (size_t)size, fp);
    fclose(fp);

    if (read != (size_t)size) { free(buf); return NULL; }

    *out_size = (size_t)size;
    return buf;
}

// ============================================================================
// Public API
// ============================================================================

EnAssetId en_asset_load(EnEngine* engine, const char* path) {
    if (!engine || !path) return EN_ASSET_INVALID;

    uint64_t hash = en_hash_fnv1a(path);

    // dedup
    EnAssetId existing = en_asset_find(engine, hash, path);
    if (existing != EN_ASSET_INVALID) {
        EnAssetSlot* s = &engine->assets[existing - 1];
        s->refcount++;
        en_log("Asset cached: %s (refcount=%u)", path, s->refcount);
        return existing;
    }

    // find handler
    const char* ext = en_ext_of(path);
    const EnAssetHandler* handler = en_find_asset_handler(engine, ext);
    if (!handler) {
        en_warn("No handler for extension .%s (path: %s)", ext, path);
        return EN_ASSET_INVALID;
    }

    // read file
    size_t size = 0;
    uint8_t* bytes = en_read_file(path, &size);
    if (!bytes) {
        en_error("Failed to read file: %s", path);
        return EN_ASSET_INVALID;
    }

    // call handler
    void* data = handler->load(engine, path, bytes, size, handler->user_data);
    free(bytes);

    if (!data) {
        en_error("Handler failed to load: %s", path);
        return EN_ASSET_INVALID;
    }

    // store
    EnAssetSlot* slot = en_asset_free_slot(engine);
    if (!slot) {
        en_error("Asset table full (max %d)", EN_MAX_ASSETS);
        handler->free(engine, data, handler->user_data);
        return EN_ASSET_INVALID;
    }

    size_t path_len = strlen(path);
    slot->path = malloc(path_len + 1);
    memcpy(slot->path, path, path_len + 1);
    slot->hash     = hash;
    slot->handler  = handler;
    slot->data     = data;
    slot->size     = size;
    slot->refcount = 1;
    slot->active   = true;

    EnAssetId id = (EnAssetId)((slot - engine->assets) + 1);
    en_log("Asset loaded: %s (id=%u, %zu bytes)", path, id, size);
    return id;
}

static EnAssetSlot* en_asset_slot(const EnEngine* engine, EnAssetId id) {
    if (id == EN_ASSET_INVALID || id > EN_MAX_ASSETS) return NULL;
    EnAssetSlot* s = (EnAssetSlot*)&engine->assets[id - 1];
    return s->active ? s : NULL;
}

void* en_asset_data(const EnEngine* engine, EnAssetId id) {
    EnAssetSlot* s = en_asset_slot(engine, id);
    return s ? s->data : NULL;
}

size_t en_asset_size(const EnEngine* engine, EnAssetId id) {
    EnAssetSlot* s = en_asset_slot(engine, id);
    return s ? s->size : 0;
}

const char* en_asset_path(const EnEngine* engine, EnAssetId id) {
    EnAssetSlot* s = en_asset_slot(engine, id);
    return s ? s->path : NULL;
}

void en_asset_release(EnEngine* engine, EnAssetId id) {
    EnAssetSlot* s = en_asset_slot(engine, id);
    if (!s) return;

    if (s->refcount > 0) s->refcount--;

    if (s->refcount == 0) {
        if (s->handler && s->handler->free)
            s->handler->free(engine, s->data, s->handler->user_data);
        free(s->path);
        memset(s, 0, sizeof(*s));
        en_log("Asset released (id=%u)", id);
    }
}
