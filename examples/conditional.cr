T choose<T>(bool condition, T first, T second) {
    return condition ? move(first) : move(second);
}

int main() {
    BoxInt selected = choose<BoxInt>(true, box_int(42), box_int(7));
    print(*selected);
    print(*selected > 10 ? "large" : "small");
    return 0;
}
