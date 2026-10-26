union Value {
    int integer;
    uint64_t wide;

    static Self from_int(int value) {
        unsafe { return (Self){ .integer = value }; }
    }
};

int main() {
    Value first = Value::from_int(42);
    Value second = first;
    unsafe {
        print(first.integer);
        second.wide = 18446744073709551615;
        print(second.wide);
    }
    print(sizeof(Value) == sizeof(uint64_t));
    return 0;
}
