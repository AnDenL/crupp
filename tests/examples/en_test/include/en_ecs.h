#ifndef EN_ECS_H
#define EN_ECS_H

#include "types.h"

#ifdef __cplusplus
extern "C" {
#endif

// ============================================================================
// Entity — стабільний handle. Копіюється по значенню.
// ============================================================================

typedef struct EnEntity {
    u32 id;
    u32 generation;
} EnEntity;

#define EN_ENTITY_NULL ((EnEntity){0, 0})

static inline bool en_entity_eq(EnEntity a, EnEntity b) {
    return a.id == b.id && a.generation == b.generation;
}

// ============================================================================
// Component metadata
// ============================================================================

typedef void (*EnDropFn)(void* ptr);

// ============================================================================
// World — opaque. Внутрішнє — у internal.h.
// ============================================================================

typedef struct EnWorld EnWorld;

EnWorld* en_world_create(void);
void     en_world_destroy(EnWorld* world);

// Component type registration. hash — стабільний (FNV-1a назви типу).
u32 en_component_register(EnWorld* world, u64 hash, usize size, usize align,
                          EnDropFn drop_fn);

// Spawning
EnEntity en_world_spawn(EnWorld* world);
void     en_world_despawn(EnWorld* world, EnEntity entity);

// Component access
bool  en_world_add_component_raw(EnWorld* world, EnEntity e, u32 comp_id,
                                 const void* data);
bool  en_world_remove_component(EnWorld* world, EnEntity e, u32 comp_id);
void* en_world_get_component(EnWorld* world, EnEntity e, u32 comp_id);

// Bundle spawn — кілька компонентів одразу (atomic, один archetype lookup)
typedef struct {
    u32         comp_id;
    const void* data;
} EnBundleEntry;

EnEntity en_world_spawn_bundle_raw(EnWorld* world,
                                   const EnBundleEntry* entries, usize count);

// Query — виклик callback для кожного чанка з усіма потрібними компонентами.
// pointers[i] відповідає comp_ids[i]. count — скільки entities у чанку.
typedef void (*EnChunkFn)(EnEntity* entities, u8** column_pointers,
                          usize count, void* user_data);

void en_world_query(EnWorld* world,
                    const u32* comp_ids, usize comp_count,
                    EnChunkFn fn, void* user_data);

// ============================================================================
// Scheduler — список систем, викликаються по черзі кожен кадр.
// ============================================================================

typedef struct EnScheduler EnScheduler;
typedef struct EnEngine EnEngine;
typedef void (*EnSystemFn)(EnEngine* engine, void* user_data);


EnScheduler* en_scheduler_create(void);
void         en_scheduler_destroy(EnScheduler* s);
void         en_scheduler_add(EnScheduler* s, EnSystemFn fn, void* user_data);
void         en_scheduler_run(EnScheduler* s, EnEngine* engine);

#ifdef __cplusplus
}
#endif

#endif // EN_ECS_H
