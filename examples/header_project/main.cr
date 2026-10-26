import "bindings.cr";

struct Counter {
    private NativeCounter* handle;

    static Self create(int value) {
        unsafe { return (Self){ .handle = native_create(value) }; }
    }

    int value() const {
        unsafe { return native_value(this->handle); }
    }

    void add(int amount) {
        unsafe { native_add(this->handle, amount); }
    }

    ~Counter() {
        unsafe { native_destroy(this->handle); }
    }
};

int main() {
    Counter first = Counter::create(40);
    first.add(2);
    Counter second = move(first);
    print(second.value());
    unsafe { print(native_limit); }
    return 0;
}
