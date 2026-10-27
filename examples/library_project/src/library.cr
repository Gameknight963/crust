extern header "../include/math.h";
extern int native_increment(int value);

int sum(int left, int right) {
    return left + right;
}

extern int add(int left, int right) = "example_add" {
    BoxInt saved = box_int(native_increment(left));
    return sum(*saved, right);
}
