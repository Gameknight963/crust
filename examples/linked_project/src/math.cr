extern int add(int left, int right) = "linked_add" {
    BoxInt saved = box_int(left);
    return *saved + right;
}
