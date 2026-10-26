#ifndef NATIVE_COUNTER_H
#define NATIVE_COUNTER_H

#include <stdint.h>

struct NativeCounter;
struct NativeCounter *counter_create(int32_t value);
int32_t counter_get(const struct NativeCounter *counter);
void counter_set(struct NativeCounter *counter, int32_t value);
void counter_destroy(struct NativeCounter *counter);

#endif
