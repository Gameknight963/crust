import "native/file_bindings.cr";
import "result.cr";

enum class FileDirection : uint8_t { None, Read, Write };
enum class FileError : uint8_t {
    OpenFailed, Closed, ReadFailed, WriteFailed, FlushFailed, SeekFailed, CloseFailed
};

tagged union FileRead {
    uint8_t byte;
    Unit end;
};

struct File {
    private void* handle;
    private FileDirection direction;

    static Result<Self, FileError> try_temporary() {
        unsafe {
            void* handle = crust_file_temporary();
            if (handle == 0) { return Result<Self, FileError>::failure(FileError::OpenFailed); }
            return Result<Self, FileError>::success(
                (Self){ .handle = handle, .direction = FileDirection::None });
        }
    }

    Result<Unit, FileError> try_close() {
        void* handle = this.handle;
        this.handle = 0;
        this.direction = FileDirection::None;
        if (handle == 0) { return Result<Unit, FileError>::success(Unit::Value); }
        unsafe {
            if (crust_file_close(handle) != 0) {
                return Result<Unit, FileError>::failure(FileError::CloseFailed);
            }
        }
        return Result<Unit, FileError>::success(Unit::Value);
    }

    Result<FileRead, FileError> try_read_byte() {
        if (!this.is_open()) { return Result<FileRead, FileError>::failure(FileError::Closed); }
        unsafe {
            if (this.direction == FileDirection::Write && crust_file_flush(this.handle) != 0) {
                return Result<FileRead, FileError>::failure(FileError::FlushFailed);
            }
            this.direction = FileDirection::Read;
            int value = crust_file_read_byte(this.handle);
            if (value >= 0) {
                return Result<FileRead, FileError>::success((FileRead){ .byte = (uint8_t)value });
            }
            if (crust_file_error(this.handle) != 0) {
                return Result<FileRead, FileError>::failure(FileError::ReadFailed);
            }
            return Result<FileRead, FileError>::success((FileRead){ .end = Unit::Value });
        }
    }

    Result<Unit, FileError> try_write_byte(uint8_t value) {
        if (!this.is_open()) { return Result<Unit, FileError>::failure(FileError::Closed); }
        unsafe {
            if (this.direction == FileDirection::Read && crust_file_sync_position(this.handle) != 0) {
                return Result<Unit, FileError>::failure(FileError::SeekFailed);
            }
            this.direction = FileDirection::Write;
            if (crust_file_write_byte(this.handle, value) != 0) {
                return Result<Unit, FileError>::failure(FileError::WriteFailed);
            }
        }
        return Result<Unit, FileError>::success(Unit::Value);
    }

    Result<Unit, FileError> try_flush() {
        if (!this.is_open()) { return Result<Unit, FileError>::failure(FileError::Closed); }
        if (this.direction != FileDirection::Write) {
            return Result<Unit, FileError>::success(Unit::Value);
        }
        unsafe {
            if (crust_file_flush(this.handle) != 0) {
                return Result<Unit, FileError>::failure(FileError::FlushFailed);
            }
        }
        return Result<Unit, FileError>::success(Unit::Value);
    }

    Result<Unit, FileError> try_rewind() {
        if (!this.is_open()) { return Result<Unit, FileError>::failure(FileError::Closed); }
        unsafe {
            if (crust_file_rewind(this.handle) != 0) {
                return Result<Unit, FileError>::failure(FileError::SeekFailed);
            }
        }
        this.direction = FileDirection::None;
        return Result<Unit, FileError>::success(Unit::Value);
    }

    static Self temporary() {
        unsafe {
            return (Self){ .handle = crust_file_temporary(), .direction = FileDirection::None };
        }
    }

    bool is_open() const { return this.handle != 0; }

    bool close() {
        Result<Unit, FileError> result = this.try_close();
        return result.is_ok();
    }

    int read_byte() {
        Result<FileRead, FileError> result = this.try_read_byte();
        if (!result.is_ok()) { return -1; }
        FileRead read = result.value();
        switch (read) {
            case byte: return read.byte;
            case end: return -1;
        }
    }

    bool write_byte(uint8_t value) {
        Result<Unit, FileError> result = this.try_write_byte(value);
        return result.is_ok();
    }

    bool flush() {
        Result<Unit, FileError> result = this.try_flush();
        return result.is_ok();
    }

    bool rewind() {
        Result<Unit, FileError> result = this.try_rewind();
        return result.is_ok();
    }

    bool eof() const {
        if (!this.is_open()) { return false; }
        unsafe { return crust_file_eof(this.handle) != 0; }
    }

    bool has_error() const {
        if (!this.is_open()) { return false; }
        unsafe { return crust_file_error(this.handle) != 0; }
    }

    ~File() { this.close(); }
};
