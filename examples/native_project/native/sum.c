#include "native_sum.h"

int32_t native_sum(const int32_t *values, size_t count)
{
    int32_t sum = 0;
    for (size_t i = 0; i < count; ++i) {
        sum += values[i];
    }
    return sum;
}
