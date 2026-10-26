import "memory.cr";

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

    const T^ get() const {
        unsafe { return ^const *this->data; }
    }

    T^ get_mut() {
        unsafe { return ^*this->data; }
    }

    ~Box() {
        unsafe {
            destroy(this->data);
            free(this->data);
        }
    }
};

