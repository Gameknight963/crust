#ifndef CRUST_EXAMPLE_NATIVE_H
#define CRUST_EXAMPLE_NATIVE_H

typedef struct NativeCounter NativeCounter;
enum NativeStatus { NativeReady = 1, NativeError = -1 };
extern const int native_limit;

NativeCounter *native_create(int value);
int native_value(const NativeCounter *counter);
enum NativeStatus native_status(const NativeCounter *counter);
void native_add(NativeCounter *counter, int amount);
void native_destroy(NativeCounter *counter);

#endif
