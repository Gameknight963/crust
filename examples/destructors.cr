struct Resource {
    int handle;

    ~Resource() {
        print(this->handle);
    }
};

int main() {
    struct Resource first = { .handle = 10 };
    struct Resource second = move(first);

    {
        struct Resource nested = { .handle = 20 };
    }

    second = (struct Resource){ .handle = 30 };
    return 0;
}
