import "result.cr";

enum class Unit : uint8_t { Value };
enum class AllocError : uint8_t { OutOfMemory, CapacityOverflow };

tagged union AllocationFailure<T> {
    T out_of_memory;
    T capacity_overflow;

    AllocError reason() const {
        switch (this) {
            case out_of_memory: return AllocError::OutOfMemory;
            case capacity_overflow: return AllocError::CapacityOverflow;
        }
    }

    T take_value(Self this) {
        switch (this) {
            case out_of_memory: return move(this.out_of_memory);
            case capacity_overflow: return move(this.capacity_overflow);
        }
    }
};
