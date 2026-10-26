int main() {
    uint32_t flags = (uint32_t)1 << 31;
    flags >>= 28;
    print(flags);

    int negative = -3;
    print(negative >> 1);
    print(-1 << 31);
    return 0;
}
