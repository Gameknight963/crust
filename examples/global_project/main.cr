import "native.cr";

int main() {
    unsafe {
        count += 1;
        print(count);
        print(limit);
        print(callback(count));
    }
    return 0;
}
