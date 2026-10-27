extern int puts(const char* text);

int main(int argc, char** argv) {
    for (int i = 1; i < argc; i++) {
        unsafe { puts(argv[i]); }
    }
    return 0;
}
