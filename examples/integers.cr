int main() {
    int8_t small = -128;
    uint16_t count = 65535;
    int64_t minimum = -9223372036854775808;
    uint64_t maximum = 18446744073709551615;

    print(small);
    print(count);
    print(minimum);
    print(maximum);

    uint32_t wrapping = 4294967295;
    wrapping += 1;
    print(wrapping);
    return 0;
}
