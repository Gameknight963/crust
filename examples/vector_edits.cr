import "container_project/src/vector.cr";
import "container_project/src/box.cr";

int main() {
    Vector<Box<int>> values = Vector<Box<int>>::create();
    values.push(Box<int>::create(10));
    values.insert(0, Box<int>::create(5));
    values.insert(1, Box<int>::create(7));

    Box<int> removed = values.remove(1);
    print(*removed.get());
    print(*values.at(0)->get());

    values.clear();
    print(values.count());
    values.insert(0, Box<int>::create(42));
    print(*values.at(0)->get());
    return 0;
}
