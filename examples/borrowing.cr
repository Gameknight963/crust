import "../lib/box.cr";

void increment(int& value)
{
    value = value + 1;
}

int read_box(const Box<int>& owner)
{
    return owner.get();
}

void replace_box(Box<int>& owner, int value)
{
    owner = Box<int>::create(value);
}

int main()
{
    int number = 41;
    increment(&number);
    print(number);

    Box<int> owner = Box<int>::create(7);
    increment(owner.get_mut());
    print(read_box(&owner));
    replace_box(&owner, 99);
    print(owner.get());

    return 0;
}
