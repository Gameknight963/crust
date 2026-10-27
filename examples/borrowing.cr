void increment(int& value)
{
    value = value + 1;
}

int read_box(const BoxInt& owner)
{
    return *owner;
}

void replace_box(BoxInt& owner, int value)
{
    owner = box_int(value);
}

int main(void)
{
    int number = 41;
    increment(&number);
    print(number);

    BoxInt owner = box_int(7);
    increment(&*owner);
    print(read_box(&owner));
    replace_box(&owner, 99);
    print(*owner);

    return 0;
}
