#ifndef EN_ECS_INTERNAL_H
#define EN_ECS_INTERNAL_H

#include "../internal.h"
#include "en_ecs.h"

// ============================================================================
// Limits
// ============================================================================

#define EN_BITMASK_WORDS       4         // 4 * 64 = 256 max components
#define EN_CHUNK_TARGET_BYTES  1024

// ============================================================================
// Bitmask
// ============================================================================

typedef struct {
    u64 bits[EN_BITMASK_WORDS];
} EnBitMask;

void en_bitmask_add(EnBitMask* m, u32 bit);
void en_bitmask_remove(EnBitMask* m, u32 bit);
bool en_bitmask_contains_all(const EnBitMask* m, const EnBitMask* other);

// ============================================================================
// Entity location
// ============================================================================

typedef struct {
    u32  archetype_idx;
    u32  chunk_idx;
    u32  row_idx;
    bool alive;
} EnEntityLocation;

// ============================================================================
// Component metadata
// ============================================================================

typedef struct {
    u32      internal_id;
    usize    size;
    usize    align;
    EnDropFn drop_fn;
} EnComponentInfo;

// ============================================================================
// Hashmap (u64 -> u32, open addressing with linear probing)
// ============================================================================

typedef struct {
    u64  hash;
    u32  value;
    bool occupied;
} EnHashEntry;

typedef struct {
    EnHashEntry* entries;
    usize        cap;
    usize        count;
} EnHashMap;

bool en_hashmap_init(EnHashMap* m, usize initial_cap);
void en_hashmap_destroy(EnHashMap* m);
bool en_hashmap_insert(EnHashMap* m, u64 hash, u32 value);  // true if new
bool en_hashmap_lookup(const EnHashMap* m, u64 hash, u32* out_value);

// ============================================================================
// Chunk (packed: one block per chunk)
// ============================================================================

typedef struct EnArchetype EnArchetype;

typedef struct EnChunk {
    u32      entity_count;
    u32      capacity;
    u8*      block;          // single aligned_alloc — owns everything below
    u8*      memory;         // points into block: component data
    u8**     column_ptrs;    // points into block: one per component
    usize*   strides;        // points into block: one per component
    EnDropFn* drop_fns;      // points into block
    EnEntity* entities;      // points into block
    usize    component_count;
    usize    layout_size;
    usize    layout_align;
} EnChunk;

EnChunk* en_chunk_create(const EnArchetype* arch, const EnWorld* world,
                         usize target_bytes);
void     en_chunk_destroy(EnChunk* chunk);

// ============================================================================
// Archetype
// ============================================================================

typedef struct EnArchetype {
    EnBitMask  mask;
    u32*       component_ids;    // sorted ascending, strictly unique
    EnDropFn*  drop_fns;         // parallel to component_ids
    usize      component_count;
    EnChunk**  chunks;
    usize      chunks_count;
    usize      chunks_cap;
    EnHashMap  add_transitions;  // comp_id -> archetype_idx
    EnHashMap  remove_transitions;
} EnArchetype;

EnArchetype* en_archetype_create(const u32* comp_ids, usize count, EnWorld* world);
void         en_archetype_destroy(EnArchetype* arch, EnWorld* world);
i32          en_archetype_column_index(const EnArchetype* arch, u32 comp_id);

// ============================================================================
// World
// ============================================================================

struct EnWorld {
    // Component registry
    EnComponentInfo* infos;
    usize            infos_count;
    usize            infos_cap;
    EnHashMap        infos_by_hash;

    // Archetypes
    EnArchetype**    archetypes;
    usize            archetypes_count;
    usize            archetypes_cap;

    // Entities
    EnEntityLocation* entities;
    u32*              current_generation;
    usize             entities_count;
    usize             entities_cap;
    u32*              free_entities;
    usize             free_count;
    usize             free_cap;
};

// ============================================================================
// Scheduler
// ============================================================================

typedef struct {
    EnSystemFn fn;
    void*      user_data;
} EnSystemSlot;

struct EnScheduler {
    EnSystemSlot* systems;
    usize         count;
    usize         cap;
};

#endif // EN_ECS_INTERNAL_H
