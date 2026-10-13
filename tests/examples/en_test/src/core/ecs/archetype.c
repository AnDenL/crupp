#include "internal.h"
#include <stdlib.h>
#include <string.h>

EnArchetype* en_archetype_create(const u32* comp_ids, usize count, EnWorld* world) {
    EnArchetype* a = calloc(1, sizeof(EnArchetype));
    if (!a) return NULL;

    a->component_count = count;
    a->component_ids = malloc(sizeof(u32) * count);
    a->drop_fns = malloc(sizeof(EnDropFn) * count);
    memcpy(a->component_ids, comp_ids, sizeof(u32) * count);

    for (usize i = 0; i < count; i++) {
        en_bitmask_add(&a->mask, comp_ids[i]);
        a->drop_fns[i] = world->infos[comp_ids[i]].drop_fn;
    }

    a->chunks_cap = 4;
    a->chunks = calloc(a->chunks_cap, sizeof(EnChunk*));

    en_hashmap_init(&a->add_transitions, 16);
    en_hashmap_init(&a->remove_transitions, 16);

    return a;
}

void en_archetype_destroy(EnArchetype* arch, EnWorld* world) {
    (void)world;
    if (!arch) return;
    for (usize i = 0; i < arch->chunks_count; i++)
        en_chunk_destroy(arch->chunks[i]);
    free(arch->chunks);
    free(arch->component_ids);
    free(arch->drop_fns);
    en_hashmap_destroy(&arch->add_transitions);
    en_hashmap_destroy(&arch->remove_transitions);
    free(arch);
}

i32 en_archetype_column_index(const EnArchetype* arch, u32 comp_id) {
    // binary search — component_ids sorted
    usize lo = 0, hi = arch->component_count;
    while (lo < hi) {
        usize mid = (lo + hi) / 2;
        if (arch->component_ids[mid] == comp_id) return (i32)mid;
        if (arch->component_ids[mid] < comp_id) lo = mid + 1;
        else hi = mid;
    }
    return -1;
}
