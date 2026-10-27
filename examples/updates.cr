int main() {
    int values[3] = { 10, 20, 30 };
    for (int index = 0; index < 3; index++) {
        print(values[index]++);
    }
    int value = 40;
    print(++value);
    print(++value);
    return 0;
}
