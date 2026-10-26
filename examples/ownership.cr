// BoxInt is a temporary built-in owner used to exercise the ownership model.
BoxInt make_box(int value)
{
    return box_int(value);
}

int consume(BoxInt owner)
{
    return *owner; // owner is freed as this function returns.
}

int main(void)
{
    BoxInt first = make_box(42);
    BoxInt second = move(first);
    print(*second);

    print(consume(move(second)));

    // Moved variables can be initialized again.
    first = box_int(7);
    print(*first);

    // first is freed automatically; the moved variables aren't freed twice.
    return 0;
}
