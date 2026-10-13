#include "internal.h"
#include <stdalign.h>
#include <stdlib.h>
#include <string.h>

static usize align_up(usize n, usize a) {
    return (n + (a - 1)) & ~(a - 1);
}

EnChunk* en_chunk_create(const EnArchetype* arch, const EnWorld* world,
                         usize target_bytes) {
    EnChunk* c = calloc(1, sizeof(EnChunk));
    if (!c) return NULL;

    usize N = arch->component_count;

    usize bytes_per_entity = 0;
    usize max_align = alignof(EnEntity);
    if (max_align < alignof(void*)) max_align = alignof(void*);
    if (max_align < alignof(usize)) max_align = alignof(usize);

    for (usize i = 0; i < N; i++) {
        const EnComponentInfo* info = &world->infos[arch->component_ids[i]];
        usize stride = align_up(info->size, info->align);
        bytes_per_entity += stride;
        if (info->align > max_align) max_align = info->align;
    }

    u32 cap = (bytes_per_entity == 0) ? 1024 :
              (u32)((target_bytes / bytes_per_entity) < 1 ? 1 : (target_bytes / bytes_per_entity));

    usize strides_off     = 0;
    usize drop_fns_off    = align_up(strides_off + N * sizeof(usize),      alignof(EnDropFn));
    usize column_ptrs_off = align_up(drop_fns_off + N * sizeof(EnDropFn),  alignof(u8*));
    usize entities_off    = align_up(column_ptrs_off + N * sizeof(u8*),    alignof(EnEntity));
    usize memory_off      = align_up(entities_off + (usize)cap * sizeof(EnEntity), max_align);

    usize memory_bytes = 0;
    for (usize i = 0; i < N; i++) {
        const EnComponentInfo* info = &world->infos[arch->component_ids[i]];
        memory_bytes = align_up(memory_bytes, info->align);
        memory_bytes += align_up(info->size, info->align) * (usize)cap;
    }

    usize total = memory_off + memory_bytes;

    total = align_up(total, max_align);
    c->block = aligned_alloc(max_align, total);
    if (!c->block) { free(c); return NULL; }
    memset(c->block, 0, total);

    // ---- Розставити вказівники в block ----
    c->strides       = (usize*)   (c->block + strides_off);
    c->drop_fns      = (EnDropFn*)(c->block + drop_fns_off);
    c->column_ptrs   = (u8**)     (c->block + column_ptrs_off);
    c->entities      = (EnEntity*)(c->block + entities_off);
    c->memory        =             c->block + memory_off;

    c->component_count = N;
    c->capacity = cap;
    c->entity_count = 0;
    c->layout_size = total;
    c->layout_align = max_align;

    // ---- column_ptrs memory ----
    usize off = 0;
    for (usize i = 0; i < N; i++) {
        const EnComponentInfo* info = &world->infos[arch->component_ids[i]];
        usize stride = align_up(info->size, info->align);
        c->strides[i]    = stride;
        c->drop_fns[i]   = info->drop_fn;
        off = align_up(off, info->align);
        c->column_ptrs[i] = c->memory + off;
        off += stride * (usize)cap;
    }

    return c;
}

void en_chunk_destroy(EnChunk* chunk) {
    if (!chunk) return;

    for (usize col = 0; col < chunk->component_count; col++) {
        EnDropFn dfn = chunk->drop_fns[col];
        if (!dfn) continue;
        for (u32 row = 0; row < chunk->entity_count; row++) {
            dfn(chunk->column_ptrs[col] + row * chunk->strides[col]);
        }
    }

    free(chunk->block);
    free(chunk);
}
