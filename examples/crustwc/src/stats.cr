import "../../../lib/file.cr";
import "../../../lib/string.cr";

enum class ReadError { Open, Read, Allocation, InvalidUtf8, Close };
struct Stats { uint64_t lines; uint64_t bytes; uint64_t characters; };

extern size_t strlen(const char* text);

unsafe Result<File, FileError> open_input(const char* path) {
    size_t length = strlen(path) + 1;
    const char[]& bounded = slice(path, length);
    return File::try_open(bounded, FileMode::Read);
}

Result<Stats, ReadError> read_stats(File& input, bool utf8) {
    Stats stats = { .lines = 0, .bytes = 0, .characters = 0 };
    ByteBuffer bytes = ByteBuffer::create();
    bool finished = false;
    while (!finished) {
        Result<FileRead, FileError> read = input.try_read_byte();
        if (!read.is_ok()) { return Result<Stats, ReadError>::failure(ReadError::Read); }
        FileRead value = read.value();
        switch (value) {
            case end: finished = true; break;
            case byte:
                stats.bytes++;
                if (value.byte == '\n') { stats.lines++; }
                if (utf8) {
                    Result<Unit, AllocError> appended = bytes.try_push(value.byte);
                    if (!appended.is_ok()) {
                        return Result<Stats, ReadError>::failure(ReadError::Allocation);
                    }
                }
                break;
        }
    }
    Result<Unit, FileError> closed = input.try_close();
    if (!closed.is_ok()) { return Result<Stats, ReadError>::failure(ReadError::Close); }
    if (utf8) {
        Result<String, ByteBuffer> decoded = String::try_from_bytes(move(bytes));
        if (!decoded.is_ok()) { return Result<Stats, ReadError>::failure(ReadError::InvalidUtf8); }
        String text = move(decoded).take_value();
        for (size_t i = 0; i < text.count(); i++) {
            if (text.is_boundary(i)) { stats.characters++; }
        }
    }
    return Result<Stats, ReadError>::success(stats);
}
