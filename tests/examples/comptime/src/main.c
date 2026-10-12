#include <stdio.h>

#if COMPTIME
#include <stdio.h>
#include <math.h>

int main(void) {
    fprintf(stderr, "⚡ [crupp:comptime] Calculating table...\n");
    printf("static const float SINS[3] = { %ff, %ff, %ff };\n",
           sinf(0.0f), sinf(1.570796f), sinf(3.141592f));
    return 0;
}
#else
static const float SINS[3];
#endif

#ifndef comptime
#define comptime(x) (x)
#endif

int main(void) {
    int val = comptime(42 * 10);
    printf("Comptime math: %d\n", val);
    printf("Generated sine 90 deg: %f\n", SINS[1]);
    return 0;
}
