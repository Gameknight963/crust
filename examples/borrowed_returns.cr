import "../lib/box.cr";

struct Item {
    Box<int> owner;

    const int& value() const {
        return this.owner.get();
    }

    int& value_mut() {
        return this.owner.get_mut();
    }
};

int main() {
    Item item = { .owner = Box<int>::create(40) };
    {
        int& value = item.value_mut();
        value += 2;
    }
    print(item.value());
    return 0;
}
