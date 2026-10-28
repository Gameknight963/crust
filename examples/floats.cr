double average(const double[]& values) {
    double total = 0;
    for (size_t i = 0; i < values.length; i++) {
        total += values[i];
    }
    return total / values.length;
}

int main() {
    double[3] values = { 1.5, 2.5, 3.5 };
    print(average(&values));
    float fraction = 0x1.8p-1f;
    print(fraction);
    return 0;
}
