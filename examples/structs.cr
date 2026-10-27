struct Item {
    int id;
    BoxInt value;
};

void increment(int& value)
{
    value = value + 1;
}

void update(struct Item& item)
{
    increment(&item.id);
    increment(&*item.value);
}

int main()
{
    struct Item first = {
        .id = 1,
        .value = box_int(42)
    };

    struct Item second = move(first);
    print(second.id);
    print(*second.value);

    update(&second);
    print(second.id);
    print(*second.value);

    return 0; // second.value is freed automatically.
}
