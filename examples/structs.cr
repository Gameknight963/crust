import "../lib/box.cr";

struct Item {
    int id;
    Box<int> value;
};

void increment(int& value)
{
    value = value + 1;
}

void update(struct Item& item)
{
    increment(&item.id);
    increment(item.value.get_mut());
}

int main()
{
    struct Item first = {
        .id = 1,
        .value = Box<int>::create(42)
    };

    struct Item second = move(first);
    print(second.id);
    print(second.value.get());

    update(&second);
    print(second.id);
    print(second.value.get());

    return 0; // second.value is freed automatically.
}
