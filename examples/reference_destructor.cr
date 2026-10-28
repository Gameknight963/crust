struct Watch {
    const int& value;

    ~Watch() {
        print(this.value);
    }
};

Watch watch(const int& value) {
    return (Watch){ .value = value };
}

int main() {
    int value = 42;
    Watch first = watch(&value);
    Watch second = move(first);
    return 0;
}
