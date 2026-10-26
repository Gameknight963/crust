typedef int Compare(const void* left, const void* right);

extern void qsort(void* values, size_t count, size_t width, Compare* compare);
extern void* bsearch(const void* key, const void* values, size_t count, size_t width, Compare* compare);
extern void* memcpy(void* destination, const void* source, size_t bytes);

unsafe int compare(const void* left, const void* right) {
    int first = *(const int*)left;
    int second = *(const int*)right;
    return (first > second) - (first < second);
}

int main() {
    int values[6] = { 9, 1, 7, 3, 5, 2 };
    int copy[6] = { 0, 0, 0, 0, 0, 0 };
    int key = 7;
    unsafe {
        qsort(&values[0], 6, sizeof(int), compare);
        memcpy(&copy[0], &values[0], sizeof(values));
        void* found = bsearch(&key, &copy[0], 6, sizeof(int), compare);
        if (found == 0) { return 1; }
        print(*(int*)found);
    }
    for (int i = 0; i < 6; i += 1) { print(copy[i]); }
    return 0;
}
