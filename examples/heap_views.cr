import "../lib/box.cr";
import "../lib/vector.cr";

struct Reading { const int& value; };

int main() {
    int first = 42;
    int second = 7;
    Vector<Reading> readings = Vector<Reading>::create();
    readings.push((Reading){.value = &const first});
    readings.push((Reading){.value = &const second});
    readings.reserve(16);
    print(readings.at(0).value);
    print(readings.at(1).value);

    Box<Reading> owner = Box<Reading>::create((Reading){.value = &const first});
    Reading extracted = move(owner).take();
    print(extracted.value);
    return 0;
}
