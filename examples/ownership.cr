import "../lib/box.cr";

Box<int> make_box(int value)
{
    return Box<int>::create(value);
}

int consume(Box<int> owner)
{
    return owner.get(); // owner is freed as this function returns.
}

int main()
{
    Box<int> first = make_box(42);
    Box<int> second = move(first);
    print(second.get());

    print(consume(move(second)));

    // Moved variables can be initialized again.
    first = Box<int>::create(7);
    print(first.get());

    // Remaining owners are freed when main returns.
    return 0;
}
