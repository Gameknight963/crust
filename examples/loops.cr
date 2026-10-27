struct Iteration {
    int number;

    ~Iteration() {
        print(this.number);
    }
};

int main() {
    int total = 0;
    for (int i = 0; i < 4; i += 1) {
        Iteration iteration = { .number = i };
        if (i == 1) { continue; }
        if (i == 3) { break; }
        total += i;
    }
    print(total);
    return 0;
}
