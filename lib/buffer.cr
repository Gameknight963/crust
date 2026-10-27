import "memory.cr";

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

    const T[]& view() const {
        unsafe { return slice(this.data, this.length); }
    }

    T[]& view_mut() {
        unsafe { return slice(this.data, this.length); }
    }

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

