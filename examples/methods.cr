import "../lib/box.cr";

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
    Box<int> value;

    int get() const {
        return this.value.get();
    }

    int finish(Self this) {
        return this.value.get();
    }
};

int main() {
    struct Counter counter = { .value = 41 };
    counter.increment();
    print(counter.get());

    struct Resource resource = { .value = Box<int>::create(7) };
    print(resource.get());
    print(move(resource).finish());
    return 0;
}
