extern long absolute(long value) = "labs";
extern long other_absolute(long value) = "labs";
typedef long (*Unary)(long);

int main() {
    Unary callback = absolute;
    unsafe {
        print(absolute(-42));
        print(other_absolute(-7));
        print(callback(-3));
    }
    return 0;
}
