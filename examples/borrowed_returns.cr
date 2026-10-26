struct Item {
    BoxInt owner;

    const int^ value() const {
        return ^const *this->owner;
    }

    int^ value_mut() {
        return ^*this->owner;
    }
};

int main() {
    Item item = { .owner = box_int(40) };
    {
        int^ value = item.value_mut();
        *value += 2;
    }
    print(*item.value());
    return 0;
}
