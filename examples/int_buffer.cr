extern void* malloc(size_t bytes);
extern void free(void* pointer);
extern void abort();

struct IntBuffer {
    private int* data;
    private size_t length;

    static Self create(size_t length) {
        if (length == 0) {
            return (Self){ .data = 0, .length = 0 };
        }
        unsafe {
            if (length > ~(size_t)0 / sizeof(int)) { abort(); }
            int* data = (int*)malloc(length * sizeof(int));
            if (data == 0) { abort(); }
            for (size_t i = 0; i < length; i += 1) {
                data[i] = 0;
            }
            return (Self){ .data = data, .length = length };
        }
    }

    size_t count() const {
        return this->length;
    }

    const int^ at(size_t index) const {
        if (index >= this->length) { unsafe { abort(); } }
        unsafe { return ^const this->data[index]; }
    }

    int^ at_mut(size_t index) {
        if (index >= this->length) { unsafe { abort(); } }
        unsafe { return ^this->data[index]; }
    }

    ~IntBuffer() {
        unsafe { free(this->data); }
    }
};

int main() {
    IntBuffer values = IntBuffer::create(3);
    *values.at_mut(0) = 10;
    *values.at_mut(1) = 20;
    *values.at_mut(2) = 30;
    for (size_t i = 0; i < values.count(); i += 1) {
        print(*values.at(i));
    }
    return 0;
}
