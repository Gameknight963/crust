struct Pair {
    int left;
    int right;
};

void add(int^ target, const int^ source) {
    *target += *source;
}

int main() {
    Pair pair = { .left = 10, .right = 20 };
    add(^pair.left, ^const pair.right);
    {
        int^ left = ^pair.left;
        int^ right = ^pair.right;
        *left += 1;
        *right += 2;
    }
    print(pair.left);
    print(pair.right);
    return 0;
}
