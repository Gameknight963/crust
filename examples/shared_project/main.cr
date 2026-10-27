extern int add(int left, int right) = "example_add";

int main() {
    unsafe {
        print(add(20, 22));
    }
    return 0;
}
