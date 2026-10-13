#include "en.h"
#include "en_ecs.h"
#include "en_components.h"
#include "en_input.h"

static u32 g_tr_id, g_sp_id;

static void move_chunk(EnEntity* ents, u8** cols, usize n, void* ud) {
    (void)ents;
    EnEngine* e = ud;
    EnTransform2D* tr = (EnTransform2D*)cols[0];

    f32 vx = 0, vy = 0;
    const f32 SPEED = 5.0f;
    f32 dt = en_delta_time(e);

    if (en_input_key_down(e, EN_KEY_LEFT))  vx -= SPEED;
    if (en_input_key_down(e, EN_KEY_RIGHT)) vx += SPEED;
    if (en_input_key_down(e, EN_KEY_UP))    vy += SPEED;
    if (en_input_key_down(e, EN_KEY_DOWN))  vy -= SPEED;

    for (usize i = 0; i < n; i++) {
        tr[i].x += vx * dt;
        tr[i].y += vy * dt;
    }
}

static void move_system(EnEngine* engine, void* ud) {
    (void)ud;
    EnWorld* w = en_world(engine);
    u32 ids[2] = { g_tr_id, g_sp_id };
    en_world_query(w, ids, 2, move_chunk, engine);
}

int main(void) {
    EnEngine* engine = en_create();
    if (!engine) return 1;
    if (!en_load_project(engine, "project.toml")) { en_destroy(engine); return 1; }
    en_destroy(engine);
    return 0;
}
