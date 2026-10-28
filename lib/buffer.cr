import "memory.cr";
import "allocation.cr";

struct Buffer<T> {
    private T* data;
    private size_t length;

    static Result<Self, AllocationFailure<T>> try_create(size_t length, T initial) {
        if (length == 0) {
            return Result<Self, AllocationFailure<T>>::success(
                (Self){ .data = 0, .length = 0 });
        }
        unsafe {
            if (length > ~(size_t)0 / sizeof(T)) {
                return Result<Self, AllocationFailure<T>>::failure(
                    (AllocationFailure<T>){ .capacity_overflow = move(initial) });
            }
            T* data = (T*)malloc(length * sizeof(T));
            if (data == 0) {
                return Result<Self, AllocationFailure<T>>::failure(
                    (AllocationFailure<T>){ .out_of_memory = move(initial) });
            }
            for (size_t i = 0; i < length; i += 1) {
                data[i] = initial;
            }
            return Result<Self, AllocationFailure<T>>::success(
                (Self){ .data = data, .length = length });
        }
    }

    static Self create(size_t length, T initial) {
        Result<Self, AllocationFailure<T>> result = Self::try_create(length, move(initial));
        if (!result.is_ok()) { unsafe { abort(); } }
        return move(result).take_value();
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

