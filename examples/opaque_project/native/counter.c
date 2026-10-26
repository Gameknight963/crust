#include "native_counter.h"
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>

struct NativeCounter {
    int32_t value;
};

struct NativeCounter *counter_create(int32_t value)
{
    struct NativeCounter *counter = malloc(sizeof(*counter));
    if (counter != NULL) { counter->value = value; }
    return counter;
}

int32_t counter_get(const struct NativeCounter *counter)
{
    return counter->value;
}

void counter_set(struct NativeCounter *counter, int32_t value)
{
    counter->value = value;
}

void counter_destroy(struct NativeCounter *counter)
{
    if (counter != NULL) {
        printf("destroy:%" PRId32 "\n", counter->value);
        free(counter);
    }
}
