import "bytes.cr";

enum class TextError : uint8_t {
    InvalidUtf8, InvalidBoundary, MissingTerminator, OutOfMemory, CapacityOverflow
};

static bool utf8_valid(const uint8_t[]& bytes) {
    size_t i = 0;
    while (i < bytes.length) {
        uint8_t first = bytes[i];
        if (first <= 0x7f) { i++; continue; }
        size_t width = 0;
        if (first >= 0xc2 && first <= 0xdf) { width = 2; }
        else if (first >= 0xe0 && first <= 0xef) { width = 3; }
        else if (first >= 0xf0 && first <= 0xf4) { width = 4; }
        else { return false; }
        if (width > bytes.length - i) { return false; }
        uint8_t second = bytes[i + 1];
        if ((first == 0xe0 && second < 0xa0)
            || (first == 0xed && second > 0x9f)
            || (first == 0xf0 && second < 0x90)
            || (first == 0xf4 && second > 0x8f)) { return false; }
        for (size_t j = 1; j < width; j++) {
            if (bytes[i + j] < 0x80 || bytes[i + j] > 0xbf) { return false; }
        }
        i += width;
    }
    return true;
}

static TextError text_allocation_error(AllocError error) {
    if (error == AllocError::OutOfMemory) { return TextError::OutOfMemory; }
    return TextError::CapacityOverflow;
}

struct String {
    private ByteBuffer bytes;

    static Self create() { return (Self){ .bytes = ByteBuffer::create() }; }

    static bool is_utf8(const uint8_t[]& source) { return utf8_valid(source); }

    static Result<Self, ByteBuffer> try_from_bytes(ByteBuffer bytes) {
        if (!utf8_valid(bytes.view())) {
            return Result<Self, ByteBuffer>::failure(move(bytes));
        }
        return Result<Self, ByteBuffer>::success((Self){ .bytes = move(bytes) });
    }

    static Result<Self, TextError> try_copy(const uint8_t[]& source) {
        if (!utf8_valid(source)) {
            return Result<Self, TextError>::failure(TextError::InvalidUtf8);
        }
        Result<ByteBuffer, AllocError> copied = ByteBuffer::try_copy(source);
        if (!copied.is_ok()) {
            return Result<Self, TextError>::failure(text_allocation_error(copied.error_value()));
        }
        return Result<Self, TextError>::success((Self){ .bytes = move(copied).take_value() });
    }

    static Result<Self, TextError> try_from_c_string(const char[]& source) {
        size_t length = 0;
        while (length < source.length && source[length] != 0) { length++; }
        if (length == source.length) {
            return Result<Self, TextError>::failure(TextError::MissingTerminator);
        }
        if (length == 0) {
            return Result<Self, TextError>::success(Self::create());
        }
        unsafe {
            const uint8_t* pointer = (const uint8_t*)&source[0];
            const uint8_t[]& bytes = slice(pointer, length);
            return Self::try_copy(bytes);
        }
    }

    size_t count() const { return this.bytes.count(); }
    const uint8_t[]& view() const { return this.bytes.view(); }
    uint8_t byte_at(size_t index) const { return this.bytes.view()[index]; }

    bool is_boundary(size_t index) const {
        if (index > this.count()) { return false; }
        if (index == this.count()) { return true; }
        return (this.byte_at(index) & 0xc0) != 0x80;
    }

    Result<Unit, TextError> try_append(const uint8_t[]& source) {
        if (!utf8_valid(source)) {
            return Result<Unit, TextError>::failure(TextError::InvalidUtf8);
        }
        Result<Unit, AllocError> appended = this.bytes.try_append(source);
        if (!appended.is_ok()) {
            return Result<Unit, TextError>::failure(text_allocation_error(appended.error_value()));
        }
        return Result<Unit, TextError>::success(Unit::Value);
    }

    Result<Unit, TextError> try_truncate(size_t length) {
        if (!this.is_boundary(length)) {
            return Result<Unit, TextError>::failure(TextError::InvalidBoundary);
        }
        this.bytes.truncate(length);
        return Result<Unit, TextError>::success(Unit::Value);
    }

    Result<Self, TextError> try_substring(size_t start, size_t length) const {
        if (start > this.count() || length > this.count() - start
            || !this.is_boundary(start) || !this.is_boundary(start + length)) {
            return Result<Self, TextError>::failure(TextError::InvalidBoundary);
        }
        Self result = Self::create();
        Result<Unit, AllocError> reserved = result.bytes.try_reserve(length);
        if (!reserved.is_ok()) {
            return Result<Self, TextError>::failure(text_allocation_error(reserved.error_value()));
        }
        for (size_t i = 0; i < length; i++) { result.bytes.push(this.byte_at(start + i)); }
        return Result<Self, TextError>::success(move(result));
    }

    void clear() { this.bytes.clear(); }
};
