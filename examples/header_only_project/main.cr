import "bindings.cr";

struct Sample {
    private NativeSample* handle;

    static Self create(uint32_t value) {
        unsafe { return (Self){.handle = sample_create(value)}; }
    }

    uint32_t value() const {
        unsafe { return sample_value(this.handle); }
    }

    ~Sample() {
        unsafe { sample_destroy(this.handle); }
    }
};

int main() {
    Sample first = Sample::create(30);
    Sample second = move(first);
    print(second.value());
    unsafe { print(sample_count); }
    return 0;
}
