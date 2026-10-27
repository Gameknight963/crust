interface Readable {
    int read() const;
};

static interface Factory {
    Self create(int value);
};

struct Counter {
    int value;
    static Self create(int value) { return (Self){ .value = value }; }
    int read() const { return this.value; }
};

int read_value<T: Readable>(const T& value) {
    return value.read();
}

T make<T: Factory>(int value) {
    return T::create(value);
}

int main() {
    Counter counter = make<Counter>(42);
    print(read_value<Counter>(&counter));
    return 0;
}
