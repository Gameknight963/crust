#define COUNT 4
#define OFFSET 10

#pragma crust begin_c
#include <stdint.h>

static int native_sum(const int *values, int count) {
    int result = 0;
    for (int i = 0; i < count; i++) result += values[i];
    return result;
}

static uint32_t native_answer(void) {
    return 32 + OFFSET;
}
#pragma crust end_c

int main() {
    int values[COUNT] = {10, 20, 5, 7};
    print(native_sum(&values[0], COUNT));
    print(native_answer());
    return 0;
}
