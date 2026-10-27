extern void* malloc(size_t bytes);
extern void free(void* pointer);
extern void abort();

struct Buffer<T> {
    private T* data;
    private size_t length;

    static Self create(size_t length, T initial) {
        if (length == 0) {
            return (Self){ .data = 0, .length = 0 };
        }
        unsafe {
            if (length > ~(size_t)0 / sizeof(T)) { abort(); }
            T* data = (T*)malloc(length * sizeof(T));
            if (data == 0) { abort(); }
            for (size_t i = 0; i < length; i += 1) {
                data[i] = initial;
            }
            return (Self){ .data = data, .length = length };
        }
    }

    size_t count() const { return this.length; }

    const T& at(size_t index) const {
        if (index >= this.length) { unsafe { abort(); } }
        unsafe { return &this.data[index]; }
    }

    T& at_mut(size_t index) {
        if (index >= this.length) { unsafe { abort(); } }
        unsafe { return &this.data[index]; }
    }

    ~Buffer() {
        unsafe { free(this.data); }
    }
};

struct Point { int x; int y; };

int main() {
    Buffer<int> values = Buffer<int>::create(3, 10);
    values.at_mut(1) = 20;
    print(values.at(0));
    print(values.at(1));

    Buffer<char> letters = Buffer<char>::create(3, 'a');
    letters.at_mut(1) = 'b';
    print(letters.at(1));

    Buffer<Point> points = Buffer<Point>::create(2, (Point){ .x = 3, .y = 4 });
    print(points.at(0).x);
    print(points.at(1).y);
    return 0;
}
