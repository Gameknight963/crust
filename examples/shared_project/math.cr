int sum(int left, int right) {
    return left + right;
}

extern int add(int left, int right) = "example_add" {
    BoxInt result = box_int(sum(left, right));
    return *result;
}
