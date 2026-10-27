import "bindings.cr";

enum FileDirection { None, Read, Write };

struct File {
    private void* handle;
    private FileDirection direction;

    static Self temporary() {
        unsafe {
            return (Self){ .handle = crust_file_temporary(), .direction = FileDirection::None };
        }
    }

    bool is_open() const { return this->handle != 0; }

    bool close() {
        void* handle = this->handle;
        this->handle = 0;
        this->direction = FileDirection::None;
        if (handle == 0) { return true; }
        unsafe { return crust_file_close(handle) == 0; }
    }

    int read_byte() {
        if (!this->is_open()) { return -1; }
        unsafe {
            // Update streams require synchronization when changing direction.
            if (this->direction == FileDirection::Write && crust_file_flush(this->handle) != 0) {
                return -1;
            }
            this->direction = FileDirection::Read;
            return crust_file_read_byte(this->handle);
        }
    }

    bool write_byte(uint8_t value) {
        if (!this->is_open()) { return false; }
        unsafe {
            if (this->direction == FileDirection::Read && crust_file_sync_position(this->handle) != 0) {
                return false;
            }
            this->direction = FileDirection::Write;
            return crust_file_write_byte(this->handle, value) == 0;
        }
    }

    bool flush() {
        if (!this->is_open()) { return false; }
        // C fflush is only valid when the most recent operation was not input.
        if (this->direction != FileDirection::Write) { return true; }
        unsafe { return crust_file_flush(this->handle) == 0; }
    }

    bool rewind() {
        if (!this->is_open()) { return false; }
        unsafe {
            if (crust_file_rewind(this->handle) != 0) { return false; }
        }
        this->direction = FileDirection::None;
        return true;
    }

    bool eof() const {
        if (!this->is_open()) { return false; }
        unsafe { return crust_file_eof(this->handle) != 0; }
    }

    bool has_error() const {
        if (!this->is_open()) { return false; }
        unsafe { return crust_file_error(this->handle) != 0; }
    }

    ~File() { this->close(); }
};
