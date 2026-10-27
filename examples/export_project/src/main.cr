extern header "../include/exports.h";

extern int add(int left, int right) = "example_add" {
    BoxInt saved = box_int(left);
    return *saved + right;
}

extern int native_call_add(int left, int right);

int main() {
    unsafe { print(native_call_add(20, 22)); }
    return 0;
}
