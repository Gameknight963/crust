struct NativeCounter;

extern NativeCounter* counter_create(int value);
extern int counter_get(const NativeCounter* counter);
extern void counter_set(NativeCounter* counter, int value);
extern void counter_destroy(NativeCounter* counter);
extern void abort();

struct Counter {
    private NativeCounter* handle;

    static Self create(int value) {
        unsafe {
            NativeCounter* handle = counter_create(value);
            if (handle == 0) { abort(); }
            return (Self){ .handle = handle };
        }
    }

    int get() const {
        unsafe { return counter_get(this.handle); }
    }

    void set(int value) {
        unsafe { counter_set(this.handle, value); }
    }

    ~Counter() {
        unsafe { counter_destroy(this.handle); }
    }
};

int main() {
    Counter original = Counter::create(1);
    original.set(42);
    Counter moved = move(original);
    print(moved.get());
    return 0;
}
