tagged union Result<T, E> {
    T ok;
    E error;

    static Self success(T value) {
        return (Self){ .ok = move(value) };
    }

    static Self failure(E value) {
        return (Self){ .error = move(value) };
    }

    bool is_ok() const {
        switch (this) {
            case ok: return true;
            case error: return false;
        }
    }

    const T& value() const { return &this.ok; }
    T& value_mut() { return &this.ok; }
    const E& error_value() const { return &this.error; }
    T take_value(Self this) { return move(this.ok); }
    E take_error(Self this) { return move(this.error); }
};
