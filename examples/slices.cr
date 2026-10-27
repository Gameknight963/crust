import "../lib/vector.cr";

int sum(const int[]& values) {
    int total = 0;
    for (size_t i = 0; i < values.length; i++) {
        total += values[i];
    }
    return total;
}

int main() {
    int[3] values = { 10, 20, 30 };
    print(sum(&values));

    Vector<int> dynamic = Vector<int>::create();
    dynamic.push(1);
    dynamic.push(2);
    {
        int[]& view = dynamic.view_mut();
        view[0] = 40;
    }
    print(sum(dynamic.view()));
    return 0;
}
