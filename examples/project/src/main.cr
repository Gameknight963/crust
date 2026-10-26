typedef int Count;

struct Counter {
    Count value;

    static Self create(Count value) {
        return (Self){ .value = value };
    }

    Count get() const {
        return this->value;
    }

    void increment() {
        this->value = this->value + 1;
    }
};

int main() {
    Counter counter = Counter::create(41);
    counter.increment();
    print(counter.get());
    return 0;
}
