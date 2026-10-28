import "vector.cr";

struct ByteBuffer {
    private Vector<uint8_t> data;

    static Self create() {
        return (Self){ .data = Vector<uint8_t>::create() };
    }

    static Result<Self, AllocError> try_copy(const uint8_t[]& source) {
        Self bytes = Self::create();
        Result<Unit, AllocError> copied = bytes.try_append(source);
        if (!copied.is_ok()) {
            return Result<Self, AllocError>::failure(copied.error_value());
        }
        return Result<Self, AllocError>::success(move(bytes));
    }

    static Self copy(const uint8_t[]& source) {
        Result<Self, AllocError> result = Self::try_copy(source);
        if (!result.is_ok()) { unsafe { abort(); } }
        return move(result).take_value();
    }

    size_t count() const { return this.data.count(); }
    const uint8_t[]& view() const { return this.data.view(); }
    uint8_t[]& view_mut() { return this.data.view_mut(); }

    Result<Unit, AllocError> try_reserve(size_t capacity) {
        return this.data.try_reserve(capacity);
    }

    Result<Unit, AllocError> try_append(const uint8_t[]& source) {
        size_t length = this.data.count();
        if (source.length > ~(size_t)0 - length) {
            return Result<Unit, AllocError>::failure(AllocError::CapacityOverflow);
        }
        if (source.length == 0) {
            return Result<Unit, AllocError>::success(Unit::Value);
        }
        size_t needed = length + source.length;
        size_t capacity = needed;
        if (length <= ~(size_t)0 / 2 && length * 2 > needed) {
            capacity = length * 2;
        }
        Result<Unit, AllocError> reserved = this.data.try_reserve(capacity);
        if (!reserved.is_ok()) {
            return Result<Unit, AllocError>::failure(reserved.error_value());
        }
        for (size_t i = 0; i < source.length; i++) {
            this.data.push(source[i]);
        }
        return Result<Unit, AllocError>::success(Unit::Value);
    }

    void append(const uint8_t[]& source) {
        Result<Unit, AllocError> result = this.try_append(source);
        if (!result.is_ok()) { unsafe { abort(); } }
    }

    void clear() { this.data.clear(); }
    void push(uint8_t value) { this.data.push(value); }

    Result<Unit, AllocError> try_push(uint8_t value) {
        Result<Unit, AllocationFailure<uint8_t>> result = this.data.try_push(value);
        if (!result.is_ok()) {
            return Result<Unit, AllocError>::failure(result.error_value().reason());
        }
        return Result<Unit, AllocError>::success(Unit::Value);
    }

    void truncate(size_t length) {
        if (length > this.data.count()) { unsafe { abort(); } }
        while (this.data.count() > length) { this.data.pop(); }
    }
};
