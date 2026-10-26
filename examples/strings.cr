typedef uint8_t Message[16];

void show(const Message^ text) {
    print(text);
}

int main() {
    Message text = "Hello, Crust!";
    show(^const text);
    text[0] = 'h';
    print(^const text);
    print("Adjacent " /* joined during parsing */ "strings");
    return 0;
}
