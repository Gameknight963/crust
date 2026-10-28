import "../lib/box.cr";

typedef int Row[3];

int sum(const Row& values) {
    int total = 0;
    for (size_t i = 0; i < 3; i += 1) {
        total += (values)[i];
    }
    return total;
}

int main() {
    Row values = { 10, 20, 30 };
    values[1] += 2;
    print(sum(&values));

    Box<int> owners[2] = { Box<int>::create(4), Box<int>::create(5) };
    print(owners[0].get());
    owners[1] = Box<int>::create(6);
    return 0;
}
