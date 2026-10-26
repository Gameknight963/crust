typedef int (*Callback)(int);

int increment(int value) { return value + 1; }
int twice(int value) { return value * 2; }

struct Handler {
    Callback apply;
};

int main() {
    Callback callback = increment;
    Handler handler = { .apply = twice };
    unsafe {
        print(callback(41));
        print((*callback)(41));
        print(handler.apply(21));
    }
    return 0;
}
