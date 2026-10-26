#ifndef NATIVE_CALLBACK_H
#define NATIVE_CALLBACK_H

#include <stdint.h>

typedef int32_t Callback(int32_t value);
int32_t native_apply(Callback *callback, int32_t value);
Callback *native_callback(void);

#endif
