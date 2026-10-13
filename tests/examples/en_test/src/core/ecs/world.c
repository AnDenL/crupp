#include "internal.h"

#include <stdlib.h>
#include <string.h>

// ============================================================================
// Forward declarations of internal helpers
// ============================================================================

static void allocate_in_archetype(EnWorld* w, u32 arch_idx, EnEntity e,
                                  u32* out_chunk, u32* out_row);
static u32  get_or_create_transition(EnWorld* w, u32 src_arch_idx,
                                     u32 comp_id, bool add);
static u32  get_or_create_archetype(EnWorld* w, const u32* comp_ids, usize count);
static EnEntity alloc_entity_id(EnWorld* w);

// ============================================================================
// Create / destroy
// ============================================================================

EnWorld* en_world_create(void) {
    EnWorld* w = calloc(1, sizeof(EnWorld));
    if (!w) return NULL;

    // Component registry
    w->infos_cap = 32;
    w->infos = calloc(w->infos_cap, sizeof(EnComponentInfo));
    en_hashmap_init(&w->infos_by_hash, 64);

    // Entity table
    w->entities_cap = 1024;
    w->entities           = calloc(w->entities_cap, sizeof(EnEntityLocation));
    w->current_generation = calloc(w->entities_cap, sizeof(u32));
    w->entities_count = 0;

    // Free list
    w->free_cap = 64;
    w->free_entities = calloc(w->free_cap, sizeof(u32));
    w->free_count = 0;

    // Archetypes — start with the empty root archetype (index 0)
    w->archetypes_cap = 16;
    w->archetypes = calloc(w->archetypes_cap, sizeof(EnArchetype*));
    w->archetypes_count = 0;

    EnArchetype* root = en_archetype_create(NULL, 0, w);
    w->archetypes[w->archetypes_count++] = root;

    return w;
}

void en_world_destroy(EnWorld* w) {
    if (!w) return;

    for (usize i = 0; i < w->archetypes_count; i++)
        en_archetype_destroy(w->archetypes[i], w);

    free(w->archetypes);
    free(w->entities);
    free(w->current_generation);
    free(w->free_entities);
    free(w->infos);
    en_hashmap_destroy(&w->infos_by_hash);
    free(w);
}

// ============================================================================
// Component registry
// ============================================================================

u32 en_component_register(EnWorld* w, u64 hash, usize size, usize align,
                          EnDropFn drop_fn) {
    // Already registered?
    u32 existing;
    if (en_hashmap_lookup(&w->infos_by_hash, hash, &existing))
        return existing;

    if (w->infos_count >= EN_BITMASK_WORDS * 64) {
        // Exceeded bitmask capacity — caller must handle error
        return UINT32_MAX;
    }

    if (w->infos_count >= w->infos_cap) {
        usize new_cap = w->infos_cap * 2;
        EnComponentInfo* p = realloc(w->infos, new_cap * sizeof(EnComponentInfo));
        if (!p) return UINT32_MAX;
        w->infos = p;
        w->infos_cap = new_cap;
    }

    u32 id = (u32)w->infos_count;
    w->infos[id].internal_id = id;
    w->infos[id].size        = size;
    w->infos[id].align       = align;
    w->infos[id].drop_fn     = drop_fn;
    w->infos_count++;

    en_hashmap_insert(&w->infos_by_hash, hash, id);
    return id;
}

// ============================================================================
// Archetype management
// ============================================================================

static u32 get_or_create_archetype(EnWorld* w, const u32* comp_ids, usize count) {
    // Linear search — fine for hundreds of archetypes.
    for (usize i = 0; i < w->archetypes_count; i++) {
        EnArchetype* a = w->archetypes[i];
        if (a->component_count != count) continue;

        bool same = true;
        for (usize j = 0; j < count; j++) {
            if (a->component_ids[j] != comp_ids[j]) { same = false; break; }
        }
        if (same) return (u32)i;
    }

    if (w->archetypes_count >= w->archetypes_cap) {
        usize new_cap = w->archetypes_cap * 2;
        EnArchetype** p = realloc(w->archetypes, new_cap * sizeof(EnArchetype*));
        if (!p) return UINT32_MAX;
        w->archetypes = p;
        w->archetypes_cap = new_cap;
    }

    EnArchetype* a = en_archetype_create(comp_ids, count, w);
    w->archetypes[w->archetypes_count] = a;
    return (u32)w->archetypes_count++;
}

static u32 get_or_create_transition(EnWorld* w, u32 src_arch_idx,
                                    u32 comp_id, bool add) {
    EnArchetype* src = w->archetypes[src_arch_idx];
    EnHashMap*   trans = add ? &src->add_transitions : &src->remove_transitions;

    u32 cached;
    if (en_hashmap_lookup(trans, comp_id, &cached))
        return cached;

    // Build the target component set: source set with comp_id added or removed.
    // Must stay sorted ascending (invariant of EnArchetype).
    usize n = src->component_count;
    u32* new_ids = malloc(sizeof(u32) * (n + 1));
    if (!new_ids) return UINT32_MAX;

    usize out = 0;
    bool inserted = false;
    for (usize i = 0; i < n; i++) {
        u32 cid = src->component_ids[i];

        if (add && !inserted && cid > comp_id) {
            new_ids[out++] = comp_id;
            inserted = true;
        }
        if (!add && cid == comp_id) {
            inserted = true;
            continue;  // skip removed component
        }
        new_ids[out++] = cid;
    }
    if (add && !inserted) new_ids[out++] = comp_id;

    u32 target_idx = get_or_create_archetype(w, new_ids, out);
    free(new_ids);

    if (target_idx == UINT32_MAX) return UINT32_MAX;

    // Cache transition in both directions.
    EnArchetype* target = w->archetypes[target_idx];
    if (add) {
        en_hashmap_insert(&src->add_transitions,    comp_id, target_idx);
        en_hashmap_insert(&target->remove_transitions, comp_id, src_arch_idx);
    } else {
        en_hashmap_insert(&src->remove_transitions, comp_id, target_idx);
        en_hashmap_insert(&target->add_transitions, comp_id, src_arch_idx);
    }
    return target_idx;
}

// ============================================================================
// Chunk allocation
// ============================================================================

static void allocate_in_archetype(EnWorld* w, u32 arch_idx, EnEntity e,
                                  u32* out_chunk, u32* out_row) {
    EnArchetype* arch = w->archetypes[arch_idx];

    u32 chunk_idx;
    if (arch->chunks_count > 0 &&
        arch->chunks[arch->chunks_count - 1]->entity_count <
        arch->chunks[arch->chunks_count - 1]->capacity) {
        chunk_idx = (u32)(arch->chunks_count - 1);
    } else {
        if (arch->chunks_count >= arch->chunks_cap) {
            usize new_cap = arch->chunks_cap * 2;
            EnChunk** p = realloc(arch->chunks, new_cap * sizeof(EnChunk*));
            if (!p) return;  // TODO: propagate error
            arch->chunks = p;
            arch->chunks_cap = new_cap;
        }
        arch->chunks[arch->chunks_count] =
            en_chunk_create(arch, w, EN_CHUNK_TARGET_BYTES);
        chunk_idx = (u32)arch->chunks_count++;
    }

    EnChunk* c = arch->chunks[chunk_idx];
    u32 row = c->entity_count;
    c->entities[row] = e;
    c->entity_count++;

    *out_chunk = chunk_idx;
    *out_row   = row;
}

// ============================================================================
// Entity id allocation
// ============================================================================

static EnEntity alloc_entity_id(EnWorld* w) {
    u32 id;
    if (w->free_count > 0) {
        id = w->free_entities[--w->free_count];
    } else {
        if (w->entities_count >= w->entities_cap) {
            usize old_cap = w->entities_cap;
            usize new_cap = old_cap * 2;

            w->entities = realloc(w->entities, new_cap * sizeof(EnEntityLocation));
            w->current_generation = realloc(w->current_generation, new_cap * sizeof(u32));

            memset(w->current_generation + old_cap, 0,
                   (new_cap - old_cap) * sizeof(u32));
            memset(w->entities + old_cap, 0,
                   (new_cap - old_cap) * sizeof(EnEntityLocation));

            w->entities_cap = new_cap;
        }
        id = (u32)w->entities_count++;
    }
    EnEntity e = { id, w->current_generation[id] };
    return e;
}

// ============================================================================
// Spawn / despawn
// ============================================================================

EnEntity en_world_spawn(EnWorld* w) {
    EnEntity e = alloc_entity_id(w);

    u32 chunk_idx, row_idx;
    allocate_in_archetype(w, 0, e, &chunk_idx, &row_idx);

    w->entities[e.id].archetype_idx = 0;
    w->entities[e.id].chunk_idx     = chunk_idx;
    w->entities[e.id].row_idx       = row_idx;
    w->entities[e.id].alive         = true;
    return e;
}

void en_world_despawn(EnWorld* w, EnEntity e) {
    if (e.id >= w->entities_count) return;

    EnEntityLocation* loc = &w->entities[e.id];
    if (!loc->alive || w->current_generation[e.id] != e.generation) return;

    EnArchetype* arch = w->archetypes[loc->archetype_idx];
    EnChunk* c = arch->chunks[loc->chunk_idx];

    // Drop component resources for this row.
    for (usize col = 0; col < c->component_count; col++) {
        EnDropFn dfn = c->drop_fns[col];
        if (dfn) dfn(c->column_ptrs[col] + loc->row_idx * c->strides[col]);
    }

    // Swap-remove with the last row.
    u32 last = c->entity_count - 1;
    if (loc->row_idx < last) {
        for (usize col = 0; col < c->component_count; col++) {
            u8*    base   = c->column_ptrs[col];
            usize  stride = c->strides[col];
            memcpy(base + loc->row_idx * stride,
                   base + last       * stride,
                   stride);
        }
        EnEntity moved = c->entities[last];
        c->entities[loc->row_idx] = moved;
        w->entities[moved.id].row_idx = loc->row_idx;
    }
    c->entity_count--;

    // Return id to the free list and bump generation.
    loc->alive = false;
    w->current_generation[e.id]++;

    if (w->free_count >= w->free_cap) {
        usize new_cap = w->free_cap * 2;
        w->free_entities = realloc(w->free_entities, new_cap * sizeof(u32));
        w->free_cap = new_cap;
    }
    w->free_entities[w->free_count++] = e.id;
}

// ============================================================================
// Component add / remove / get
// ============================================================================

bool en_world_add_component_raw(EnWorld* w, EnEntity e, u32 comp_id,
                                const void* data) {
    if (e.id >= w->entities_count) return false;
    EnEntityLocation* loc = &w->entities[e.id];
    if (!loc->alive || w->current_generation[e.id] != e.generation) return false;

    EnArchetype* src_arch = w->archetypes[loc->archetype_idx];
    i32 existing_col = en_archetype_column_index(src_arch, comp_id);
    EnChunk* src_c = src_arch->chunks[loc->chunk_idx];

    // Already has this component — overwrite in place.
    if (existing_col >= 0) {
        memcpy(src_c->column_ptrs[existing_col] + loc->row_idx * src_c->strides[existing_col],
               data, w->infos[comp_id].size);
        return true;
    }

    u32 target_idx = get_or_create_transition(w, loc->archetype_idx, comp_id, true);
    if (target_idx == UINT32_MAX) return false;

    u32 new_chunk, new_row;
    allocate_in_archetype(w, target_idx, e, &new_chunk, &new_row);

    EnArchetype* tgt_arch = w->archetypes[target_idx];
    EnChunk*     tgt_c    = tgt_arch->chunks[new_chunk];

    // Copy all shared components.
    for (usize i = 0; i < src_arch->component_count; i++) {
        u32 cid = src_arch->component_ids[i];
        i32 tcol = en_archetype_column_index(tgt_arch, cid);
        memcpy(tgt_c->column_ptrs[tcol] + new_row * tgt_c->strides[tcol],
               src_c->column_ptrs[i]    + loc->row_idx * src_c->strides[i],
               w->infos[cid].size);
    }

    // Copy the newly added component.
    i32 new_col = en_archetype_column_index(tgt_arch, comp_id);
    memcpy(tgt_c->column_ptrs[new_col] + new_row * tgt_c->strides[new_col],
           data, w->infos[comp_id].size);

    // Swap-remove from source chunk.
    u32 last = src_c->entity_count - 1;
    if (loc->row_idx < last) {
        for (usize col = 0; col < src_c->component_count; col++) {
            u8*   base   = src_c->column_ptrs[col];
            usize stride = src_c->strides[col];
            memcpy(base + loc->row_idx * stride,
                   base + last       * stride,
                   stride);
        }
        EnEntity moved = src_c->entities[last];
        src_c->entities[loc->row_idx] = moved;
        w->entities[moved.id].row_idx = loc->row_idx;
    }
    src_c->entity_count--;

    loc->archetype_idx = target_idx;
    loc->chunk_idx     = new_chunk;
    loc->row_idx       = new_row;
    return true;
}

bool en_world_remove_component(EnWorld* w, EnEntity e, u32 comp_id) {
    if (e.id >= w->entities_count) return false;
    EnEntityLocation* loc = &w->entities[e.id];
    if (!loc->alive || w->current_generation[e.id] != e.generation) return false;

    EnArchetype* src_arch = w->archetypes[loc->archetype_idx];
    i32 src_col = en_archetype_column_index(src_arch, comp_id);
    if (src_col < 0) return false;

    EnChunk* src_c = src_arch->chunks[loc->chunk_idx];

    // Drop the removed component's resources for this row.
    if (src_c->drop_fns[src_col])
        src_c->drop_fns[src_col](src_c->column_ptrs[src_col] +
                                 loc->row_idx * src_c->strides[src_col]);

    u32 target_idx = get_or_create_transition(w, loc->archetype_idx, comp_id, false);
    if (target_idx == UINT32_MAX) return false;

    u32 new_chunk, new_row;
    allocate_in_archetype(w, target_idx, e, &new_chunk, &new_row);

    EnArchetype* tgt_arch = w->archetypes[target_idx];
    EnChunk*     tgt_c    = tgt_arch->chunks[new_chunk];

    // Copy all components except the one being removed.
    for (usize i = 0; i < src_arch->component_count; i++) {
        u32 cid = src_arch->component_ids[i];
        if (cid == comp_id) continue;

        i32 tcol = en_archetype_column_index(tgt_arch, cid);
        memcpy(tgt_c->column_ptrs[tcol] + new_row * tgt_c->strides[tcol],
               src_c->column_ptrs[i]    + loc->row_idx * src_c->strides[i],
               w->infos[cid].size);
    }

    // Swap-remove from source chunk.
    u32 last = src_c->entity_count - 1;
    if (loc->row_idx < last) {
        for (usize col = 0; col < src_c->component_count; col++) {
            u8*   base   = src_c->column_ptrs[col];
            usize stride = src_c->strides[col];
            memcpy(base + loc->row_idx * stride,
                   base + last       * stride,
                   stride);
        }
        EnEntity moved = src_c->entities[last];
        src_c->entities[loc->row_idx] = moved;
        w->entities[moved.id].row_idx = loc->row_idx;
    }
    src_c->entity_count--;

    loc->archetype_idx = target_idx;
    loc->chunk_idx     = new_chunk;
    loc->row_idx       = new_row;
    return true;
}

void* en_world_get_component(EnWorld* w, EnEntity e, u32 comp_id) {
    if (e.id >= w->entities_count) return NULL;

    EnEntityLocation* loc = &w->entities[e.id];
    if (!loc->alive || w->current_generation[e.id] != e.generation) return NULL;

    EnArchetype* arch = w->archetypes[loc->archetype_idx];
    i32 col = en_archetype_column_index(arch, comp_id);
    if (col < 0) return NULL;

    EnChunk* c = arch->chunks[loc->chunk_idx];
    return c->column_ptrs[col] + loc->row_idx * c->strides[col];
}

// ============================================================================
// Bundle spawn
// ============================================================================

EnEntity en_world_spawn_bundle_raw(EnWorld* w, const EnBundleEntry* entries,
                                   usize count) {
    EnEntity e = alloc_entity_id(w);

    // Sort component ids — EnArchetype requires ascending order.
    u32* ids = malloc(sizeof(u32) * count);
    for (usize i = 0; i < count; i++) ids[i] = entries[i].comp_id;
    for (usize i = 1; i < count; i++) {
        u32 key = ids[i];
        usize j = i;
        while (j > 0 && ids[j - 1] > key) { ids[j] = ids[j - 1]; j--; }
        ids[j] = key;
    }

    u32 arch_idx = get_or_create_archetype(w, ids, count);
    free(ids);
    if (arch_idx == UINT32_MAX) return EN_ENTITY_NULL;

    u32 chunk_idx, row_idx;
    allocate_in_archetype(w, arch_idx, e, &chunk_idx, &row_idx);

    EnArchetype* arch = w->archetypes[arch_idx];
    EnChunk*     c    = arch->chunks[chunk_idx];

    for (usize i = 0; i < count; i++) {
        u32 cid  = entries[i].comp_id;
        i32 col  = en_archetype_column_index(arch, cid);
        memcpy(c->column_ptrs[col] + row_idx * c->strides[col],
               entries[i].data, w->infos[cid].size);
    }

    w->entities[e.id].archetype_idx = arch_idx;
    w->entities[e.id].chunk_idx     = chunk_idx;
    w->entities[e.id].row_idx       = row_idx;
    w->entities[e.id].alive         = true;
    return e;
}

// ============================================================================
// Query
// ============================================================================

void en_world_query(EnWorld* w, const u32* comp_ids, usize comp_count,
                    EnChunkFn fn, void* user_data) {
    EnBitMask req = {0};
    for (usize i = 0; i < comp_count; i++)
        en_bitmask_add(&req, comp_ids[i]);

    u8** pointers = malloc(sizeof(u8*) * comp_count);

    for (usize ai = 0; ai < w->archetypes_count; ai++) {
        EnArchetype* arch = w->archetypes[ai];
        if (!en_bitmask_contains_all(&arch->mask, &req)) continue;

        // Resolve column indices once per archetype.
        i32* cols = malloc(sizeof(i32) * comp_count);
        bool ok = true;
        for (usize i = 0; i < comp_count; i++) {
            cols[i] = en_archetype_column_index(arch, comp_ids[i]);
            if (cols[i] < 0) { ok = false; break; }
        }
        if (!ok) { free(cols); continue; }

        for (usize ci = 0; ci < arch->chunks_count; ci++) {
            EnChunk* c = arch->chunks[ci];
            if (c->entity_count == 0) continue;

            for (usize i = 0; i < comp_count; i++)
                pointers[i] = c->column_ptrs[cols[i]];

            fn(c->entities, pointers, c->entity_count, user_data);
        }
        free(cols);
    }
    free(pointers);
}
