import "memory.cr";
import "allocation.cr";

struct Vector<T> {
    private T* data;
    private size_t length;
    private size_t capacity;

    static Self create() {
        return (Self){ .data = 0, .length = 0, .capacity = 0 };
    }

    size_t count() const { return this.length; }

    const T[]& view() const {
        unsafe { return slice(this.data, this.length); }
    }

    T[]& view_mut() {
        unsafe { return slice(this.data, this.length); }
    }

    Result<Unit, AllocError> try_reserve(size_t capacity) {
        if (capacity <= this.capacity) {
            return Result<Unit, AllocError>::success(Unit::Value);
        }
        unsafe {
            if (capacity > ~(size_t)0 / sizeof(T)) {
                return Result<Unit, AllocError>::failure(AllocError::CapacityOverflow);
            }
            T* next = (T*)malloc(capacity * sizeof(T));
            if (next == 0) {
                return Result<Unit, AllocError>::failure(AllocError::OutOfMemory);
            }
            for (size_t i = 0; i < this.length; i += 1) {
                init(next + i, move(this.data[i]));
            }
            free(this.data);
            this.data = next;
            this.capacity = capacity;
        }
        return Result<Unit, AllocError>::success(Unit::Value);
    }

    Result<Unit, AllocationFailure<T>> try_push(T value) {
        size_t index = this.length;
        return this.try_insert(index, move(value));
    }

    Result<Unit, AllocationFailure<T>> try_insert(size_t index, T value) {
        if (index > this.length) { unsafe { abort(); } }
        if (this.length == this.capacity) {
            size_t next = 2;
            if (this.capacity > 0) {
                if (this.capacity > ~(size_t)0 / 2) {
                    return Result<Unit, AllocationFailure<T>>::failure(
                        (AllocationFailure<T>){ .capacity_overflow = move(value) });
                }
                next = this.capacity * 2;
            }
            Result<Unit, AllocError> reserved = this.try_reserve(next);
            if (!reserved.is_ok()) {
                if (reserved.error_value() == AllocError::CapacityOverflow) {
                    return Result<Unit, AllocationFailure<T>>::failure(
                        (AllocationFailure<T>){ .capacity_overflow = move(value) });
                }
                return Result<Unit, AllocationFailure<T>>::failure(
                    (AllocationFailure<T>){ .out_of_memory = move(value) });
            }
        }
        unsafe {
            for (size_t i = this.length; i > index; i -= 1) {
                init(this.data + i, move(this.data[i - 1]));
            }
            init(this.data + index, move(value));
        }
        this.length += 1;
        return Result<Unit, AllocationFailure<T>>::success(Unit::Value);
    }

    void reserve(size_t capacity) {
        Result<Unit, AllocError> result = this.try_reserve(capacity);
        if (!result.is_ok()) { unsafe { abort(); } }
    }

    void push(T value) {
        size_t index = this.length;
        this.insert(index, move(value));
    }

    void insert(size_t index, T value) {
        Result<Unit, AllocationFailure<T>> result = this.try_insert(index, move(value));
        if (!result.is_ok()) { unsafe { abort(); } }
    }

    T pop() {
        if (this.length == 0) { unsafe { abort(); } }
        unsafe {
            T value = move(this.data[this.length - 1]);
            this.length -= 1;
            return move(value);
        }
    }

    T remove(size_t index) {
        if (index >= this.length) { unsafe { abort(); } }
        unsafe {
            T value = move(this.data[index]);
            size_t length = this.length - 1;
            for (size_t i = index; i < length; i += 1) {
                init(this.data + i, move(this.data[i + 1]));
            }
            this.length = length;
            return move(value);
        }
    }

    void clear() {
        unsafe {
            for (size_t i = this.length; i > 0; i -= 1) {
                destroy(this.data + (i - 1));
            }
        }
        this.length = 0;
    }

    void swap(size_t first, size_t second) {
        if (first >= this.length || second >= this.length) { unsafe { abort(); } }
        if (first == second) { return; }
        unsafe {
            T value = move(this.data[first]);
            init(this.data + first, move(this.data[second]));
            init(this.data + second, move(value));
        }
    }

    const T& at(size_t index) const {
        if (index >= this.length) { unsafe { abort(); } }
        unsafe { return &this.data[index]; }
    }

    T& at_mut(size_t index) {
        if (index >= this.length) { unsafe { abort(); } }
        unsafe { return &this.data[index]; }
    }

    ~Vector() {
        this.clear();
        unsafe {
            free(this.data);
        }
    }
};
