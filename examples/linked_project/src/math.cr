import "../../../lib/box.cr";

extern int add(int left, int right) = "linked_add" {
    Box<int> saved = Box<int>::create(left);
    return saved.get() + right;
}
