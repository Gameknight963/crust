import "memory.cr";
import "allocation.cr";

struct Box<T> {
    private T* data;

    static Result<Self, AllocationFailure<T>> try_create(T value) borrows(value) {
        unsafe {
            T* data = (T*)malloc(sizeof(T));
            if (data == 0) {
                return Result<Self, AllocationFailure<T>>::failure(
                    (AllocationFailure<T>){ .out_of_memory = move(value) });
            }
            init(data, move(value));
            return Result<Self, AllocationFailure<T>>::success((Self){ .data = data });
        }
    }

    static Self create(T value) borrows(value) {
        Result<Self, AllocationFailure<T>> result = Self::try_create(move(value));
        if (!result.is_ok()) { unsafe { abort(); } }
        return move(result).take_value();
    }

    const T& get() const {
        unsafe { return &*this.data; }
    }

    T& get_mut() {
        unsafe { return &*this.data; }
    }

    T take(Self this) borrows(this) {
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

