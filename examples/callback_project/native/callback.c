#include "native_callback.h"

static int32_t identity(int32_t value)
{
    return value;
}

int32_t native_apply(Callback *callback, int32_t value)
{
    return callback(value);
}

Callback *native_callback(void)
{
    return identity;
}
