typedef int (*Callback)(int value);
typedef int (*Apply)(Callback callback, int value);

extern int native_apply(Callback callback, int value);
extern Callback native_callback();

int increment(int value) {
    return value + 1;
}

int main() {
    Apply apply = &native_apply;
    unsafe {
        print(native_apply(increment, 41));
        print(apply(increment, 41));
        Callback callback = native_callback();
        print(callback(41));
    }
    return 0;
}
