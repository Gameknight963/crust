struct Counter {
    int value;

    int get() const {
        return this.value;
    }

    void increment() {
        this.value = this.value + 1;
    }
};

struct Resource {
    BoxInt value;

    int get() const {
        return *this.value;
    }

    int finish(Self this) {
        return *this.value;
    }
};

int main() {
    struct Counter counter = { .value = 41 };
    counter.increment();
    print(counter.get());

    struct Resource resource = { .value = box_int(7) };
    print(resource.get());
    print(move(resource).finish());
    return 0;
}
