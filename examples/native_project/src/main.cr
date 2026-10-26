extern int native_sum(const int* values, size_t count);

int main() {
    int values[4] = { 10, 20, 30, 40 };
    unsafe {
        print(native_sum(&values[0], 4));
    }
    return 0;
}
