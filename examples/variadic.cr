extern int snprintf(char* destination, size_t capacity, const char* format, ...);
typedef int (*Formatter)(char*, size_t, const char*, ...);

enum Small : uint8_t { Ready = 7 };

int main() {
    char format[9] = "%d:%d:%d";
    char output[32] = "";
    uint8_t byte = 255;
    bool enabled = true;
    unsafe {
        print(snprintf(&output[0], sizeof(output), &format[0], byte, enabled, Small::Ready));
        print(output);
        Formatter formatter = snprintf;
        print(formatter(&output[0], sizeof(output), &format[0], -1, (uint16_t)65535, 0));
        print(output);
    }
    return 0;
}
