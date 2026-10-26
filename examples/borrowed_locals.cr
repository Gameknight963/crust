int main() {
    BoxInt owner = box_int(40);
    {
        int^ value = ^*owner;
        *value += 1;
        {
            const int^ view = ^const *value;
            print(*view);
        }
        *value += 1;
    }
    print(*owner);
    return 0;
}
