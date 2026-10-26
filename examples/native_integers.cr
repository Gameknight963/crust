extern long labs(long value);
extern long long llabs(long long value);
extern unsigned long strtoul(const char* text, char** end, int base);

int main() {
    signed char small = -128;
    unsigned short int half = 65535;
    long native = 2147483647;
    long long wide = -9223372036854775807;
    unsigned long mask = ~(unsigned long)0;
    mask += 1;
    print(small);
    print(half);
    print(mask);
    char text[11] = "2147483647";
    unsafe {
        print(labs(-native));
        print(llabs(wide));
        print(strtoul(&text[0], 0, 10));
    }
    return 0;
}
