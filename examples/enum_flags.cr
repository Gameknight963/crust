enum Access : uint32_t {
    Read = 1 << 0,
    Write = 1 << 1,
    ReadWrite = Read | Access::Write,
    High = (uint32_t)1 << 31,
};

int main() {
    print((uint32_t)ReadWrite);
    print((uint32_t)High);
    return 0;
}
