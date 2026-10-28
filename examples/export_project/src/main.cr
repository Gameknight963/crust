import "../../../lib/box.cr";

extern header "../include/exports.h";

extern int add(int left, int right) = "example_add" {
    Box<int> saved = Box<int>::create(left);
    return saved.get() + right;
}

extern int native_call_add(int left, int right);

int main() {
    unsafe { print(native_call_add(20, 22)); }
    return 0;
}
