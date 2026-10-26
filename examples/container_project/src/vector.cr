import "memory.cr";

struct Vector<T> {
    private T* data;
    private size_t length;
    private size_t capacity;

    static Self create() {
        return (Self){ .data = 0, .length = 0, .capacity = 0 };
    }

    size_t count() const { return this->length; }

    void reserve(size_t capacity) {
        if (capacity <= this->capacity) { return; }
        unsafe {
            if (capacity > ~(size_t)0 / sizeof(T)) { abort(); }
            T* next = (T*)malloc(capacity * sizeof(T));
            if (next == 0) { abort(); }
            for (size_t i = 0; i < this->length; i += 1) {
                init(next + i, move(this->data[i]));
            }
            free(this->data);
            this->data = next;
            this->capacity = capacity;
        }
    }

    void push(T value) {
        if (this->length == this->capacity) {
            size_t next = 2;
            if (this->capacity > 0) {
                if (this->capacity > ~(size_t)0 / 2) { unsafe { abort(); } }
                next = this->capacity * 2;
            }
            this->reserve(next);
        }
        unsafe { init(this->data + this->length, move(value)); }
        this->length += 1;
    }

    T pop() {
        if (this->length == 0) { unsafe { abort(); } }
        unsafe {
            T value = move(this->data[this->length - 1]);
            this->length -= 1;
            return move(value);
        }
    }

    const T^ at(size_t index) const {
        if (index >= this->length) { unsafe { abort(); } }
        unsafe { return ^const this->data[index]; }
    }

    T^ at_mut(size_t index) {
        if (index >= this->length) { unsafe { abort(); } }
        unsafe { return ^this->data[index]; }
    }

    ~Vector() {
        unsafe {
            for (size_t i = this->length; i > 0; i -= 1) {
                destroy(this->data + (i - 1));
            }
            free(this->data);
        }
    }
};
