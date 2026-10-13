#include <webgpu/wgpu.h>
#include <stdio.h>

int main(void) {
    WGPUInstance instance = wgpuCreateInstance(NULL);
    if (!instance) {
        printf("FAILED\n");
        return 1;
    }
    printf("OK: instance = %p\n", (void*)instance);
    wgpuInstanceRelease(instance);
    return 0;
}
