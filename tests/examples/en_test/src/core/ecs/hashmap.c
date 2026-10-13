#include "internal.h"
#include <stdlib.h>
#include <string.h>

static usize next_pow2(usize n) {
    usize p = 1;
    while (p < n) p <<= 1;
    return p;
}

bool en_hashmap_init(EnHashMap* m, usize initial_cap) {
    m->cap = next_pow2(initial_cap < 8 ? 8 : initial_cap);
    m->count = 0;
    m->entries = calloc(m->cap, sizeof(EnHashEntry));
    return m->entries != NULL;
}

void en_hashmap_destroy(EnHashMap* m) {
    free(m->entries);
    m->entries = NULL;
    m->cap = 0;
    m->count = 0;
}

/// returns true if inserted new, false if key existed (value overwritten)
bool en_hashmap_insert(EnHashMap* m, u64 hash, u32 value) {
    // Grow if load factor > 0.75
    if (m->count * 4 >= m->cap * 3) {
        usize new_cap = m->cap * 2;
        EnHashEntry* new_entries = calloc(new_cap, sizeof(EnHashEntry));
        if (!new_entries) return false;

        for (usize i = 0; i < m->cap; i++) {
            if (!m->entries[i].occupied) continue;
            u64 h = m->entries[i].hash;
            usize slot = h & (new_cap - 1);
            while (new_entries[slot].occupied) slot = (slot + 1) & (new_cap - 1);
            new_entries[slot] = m->entries[i];
        }
        free(m->entries);
        m->entries = new_entries;
        m->cap = new_cap;
    }

    usize slot = hash & (m->cap - 1);
    while (m->entries[slot].occupied) {
        if (m->entries[slot].hash == hash) {
            m->entries[slot].value = value;
            return false;
        }
        slot = (slot + 1) & (m->cap - 1);
    }
    m->entries[slot].hash = hash;
    m->entries[slot].value = value;
    m->entries[slot].occupied = true;
    m->count++;
    return true;
}

bool en_hashmap_lookup(const EnHashMap* m, u64 hash, u32* out_value) {
    if (m->cap == 0) return false;
    usize slot = hash & (m->cap - 1);
    while (m->entries[slot].occupied) {
        if (m->entries[slot].hash == hash) {
            if (out_value) *out_value = m->entries[slot].value;
            return true;
        }
        slot = (slot + 1) & (m->cap - 1);
    }
    return false;
}
