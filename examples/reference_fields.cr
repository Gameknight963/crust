struct View {
    const int& value;
};

struct Counter {
    int& value;

    void increment() {
        this.value++;
    }
};

View view(const int& value) {
    return (View){ .value = value };
}

int main() {
    int value = 41;
    {
        Counter counter = { .value = &value };
        counter.increment();
    }
    {
        View first = view(&value);
        View second = first;
        print(second.value);
    }
    return 0;
}
