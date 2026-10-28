import "../lib/box.cr";

T choose<T>(bool condition, T first, T second) {
    return condition ? move(first) : move(second);
}

int main() {
    Box<int> selected = choose<Box<int>>(true, Box<int>::create(42), Box<int>::create(7));
    print(selected.get());
    print(selected.get() > 10 ? "large" : "small");
    return 0;
}
