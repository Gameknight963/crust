extern void* malloc(size_t bytes);
extern void free(void* value);
extern void abort();

struct HeapInt {
    private int* pointer;

    static Self create(int value) {
        unsafe {
            int* pointer = (int*)malloc(sizeof(int));
            if (pointer == 0) { abort(); }
            *pointer = value;
            return (Self){ .pointer = pointer };
        }
    }

    const int^ value() const {
        unsafe { return ^const *this->pointer; }
    }

    int^ value_mut() {
        unsafe { return ^*this->pointer; }
    }

    ~HeapInt() {
        unsafe { free(this->pointer); }
    }
};

int main() {
    HeapInt first = HeapInt::create(40);
    {
        int^ value = first.value_mut();
        *value += 2;
    }
    HeapInt second = move(first);
    print(*second.value());
    return 0;
}
