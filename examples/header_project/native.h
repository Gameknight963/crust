#ifndef CRUST_EXAMPLE_NATIVE_H
#define CRUST_EXAMPLE_NATIVE_H

typedef struct NativeCounter NativeCounter;
extern const int native_limit;

NativeCounter *native_create(int value);
int native_value(const NativeCounter *counter);
void native_add(NativeCounter *counter, int amount);
void native_destroy(NativeCounter *counter);

#endif
