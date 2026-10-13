#include "internal.h"

#include <stdlib.h>

struct EnScheduler {
    EnSystemSlot* systems;
    usize         count;
    usize         cap;
};

EnScheduler* en_scheduler_create(void) {
    EnScheduler* s = calloc(1, sizeof(EnScheduler));
    if (!s) return NULL;

    s->cap = 16;
    s->systems = calloc(s->cap, sizeof(EnSystemSlot));
    return s;
}

void en_scheduler_destroy(EnScheduler* s) {
    if (!s) return;
    free(s->systems);
    free(s);
}

void en_scheduler_add(EnScheduler* s, EnSystemFn fn, void* user_data) {
    if (s->count >= s->cap) {
        usize new_cap = s->cap * 2;
        s->systems = realloc(s->systems, new_cap * sizeof(EnSystemSlot));
        s->cap = new_cap;
    }
    s->systems[s->count].fn        = fn;
    s->systems[s->count].user_data = user_data;
    s->count++;
}

void en_scheduler_run(EnScheduler* s, EnEngine* engine) {
    for (usize i = 0; i < s->count; i++)
        s->systems[i].fn(engine, s->systems[i].user_data);
}
