extern void* malloc(size_t bytes);
extern void free(void* pointer);
extern void abort();

struct Box<T> {
    private T* data;

    static Self create(T value) {
        unsafe {
            T* data = (T*)malloc(sizeof(T));
            if (data == 0) { abort(); }
            init(data, move(value));
            return (Self){ .data = data };
        }
    }

    const T& get() const {
        unsafe { return &*this.data; }
    }

    T& get_mut() {
        unsafe { return &*this.data; }
    }

    T take(Self this) {
        unsafe {
            T value = move(*this.data);
            free(this.data);
            this.data = 0;
            return move(value);
        }
    }

    ~Box() {
        unsafe {
            if (this.data != 0) { destroy(this.data); }
            free(this.data);
        }
    }
};

int main() {
    Box<int> number = Box<int>::create(42);
    print(number.get());
    Box<BoxInt> inner = Box<BoxInt>::create(box_int(7));
    Box<Box<BoxInt>> outer = Box<Box<BoxInt>>::create(move(inner));
    print(*outer.get().get());
    return 0;
}
