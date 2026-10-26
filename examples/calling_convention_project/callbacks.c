typedef int (__vectorcall *Callback)(int);

int __vectorcall apply_native(Callback callback, int value) {
    return callback(value) + 1;
}

static int __vectorcall triple(int value) {
    return value * 3;
}

Callback __stdcall native_callback(void) {
    return triple;
}
