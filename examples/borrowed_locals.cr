import "../lib/box.cr";

int main() {
    Box<int> owner = Box<int>::create(40);
    {
        int& value = owner.get_mut();
        value += 1;
        {
            const int& view = &value;
            print(view);
        }
        value += 1;
    }
    print(owner.get());
    return 0;
}
