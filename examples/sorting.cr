import "container_project/src/sort.cr";

struct Number {
    private BoxInt value;

    static Self create(int value) { return (Self){ .value = box_int(value) }; }
    int get() const { return *this->value; }

    int compare(const Self^ other) const {
        int first = *this->value;
        int second = *other->value;
        return (first > second) - (first < second);
    }
};

int main() {
    Vector<Number> values = Vector<Number>::create();
    values.push(Number::create(3));
    values.push(Number::create(1));
    values.push(Number::create(2));
    values.push(Number::create(1));
    sort<Number>(^values);
    for (size_t i = 0; i < values.count(); i += 1) { print(values.at(i)->get()); }
    return 0;
}
