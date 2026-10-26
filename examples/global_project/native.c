int native_count = 41;
const int native_limit = 100;

static int increment(int value) {
    return value + 1;
}

int (*native_callback)(int) = increment;
