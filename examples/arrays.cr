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

    BoxInt owners[2] = { box_int(4), box_int(5) };
    print(*owners[0]);
    owners[1] = box_int(6);
    return 0;
}
