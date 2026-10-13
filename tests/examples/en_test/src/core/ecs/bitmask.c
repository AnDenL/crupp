#include "internal.h"
// #include <string.h>

void en_bitmask_add(EnBitMask* m, u32 bit) {
    m->bits[bit / 64] |= 1ULL << (bit % 64);
}

void en_bitmask_remove(EnBitMask* m, u32 bit) {
    m->bits[bit / 64] &= ~(1ULL << (bit % 64));
}

bool en_bitmask_contains_all(const EnBitMask* m, const EnBitMask* other) {
    for (usize i = 0; i < EN_BITMASK_WORDS; i++) {
        if ((m->bits[i] & other->bits[i]) != other->bits[i])
            return false;
    }
    return true;
}
