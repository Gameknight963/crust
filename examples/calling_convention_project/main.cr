typedef int (__vectorcall *Callback)(int);

extern int __vectorcall apply_native(Callback callback, int value);
extern Callback __stdcall native_callback();

T __vectorcall identity<T>(T value) {
    return value;
}

struct Operations {
    int unused;

    static int __vectorcall twice(int value) {
        return value * 2;
    }
};

int main() {
    Callback first = identity<int>;
    Callback second = Operations::twice;
    unsafe {
        print(apply_native(first, 40));
        print(apply_native(second, 21));
        Callback third = native_callback();
        print(third(14));
    }
    return 0;
}
