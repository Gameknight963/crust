void advance(volatile int^ value) {
    *value += 1;
}

int main() {
    int counter = 41;
    unsafe {
        volatile int* register_ = &counter;
        advance(^*register_);
        const volatile int^ view = ^const *register_;
        print(*view);
    }
    return 0;
}
