#include "native.h"
#include <stdlib.h>
#include <limits.h>

struct NativeCounter {
    int value;
};

NativeCounter *native_create(int value) {
    NativeCounter *counter = malloc(sizeof(*counter));
    if (counter == NULL) { abort(); }
    counter->value = value;
    return counter;
}

int native_value(const NativeCounter *counter) {
    return counter->value;
}

void native_add(NativeCounter *counter, int amount) {
    if ((amount > 0 && counter->value > INT_MAX - amount) ||
        (amount < 0 && counter->value < INT_MIN - amount)) {
        abort();
    }
    counter->value += amount;
}

void native_destroy(NativeCounter *counter) {
    free(counter);
}
