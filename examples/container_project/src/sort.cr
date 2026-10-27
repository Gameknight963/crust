import "vector.cr";

interface Ordered {
    int compare(const Self& other) const;
};

void sort<T: Ordered>(Vector<T>& values) {
    size_t length = values.count();
    for (size_t i = 1; i < length; i += 1) {
        size_t position = i;
        while (position > 0 && values.at(position - 1).compare(values.at(position)) > 0) {
            values.swap(position - 1, position);
            position -= 1;
        }
    }
}
