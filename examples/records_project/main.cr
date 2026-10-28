import "bindings.cr";

void increment(int& value) { value++; }

int main() {
    Sample samples[2] = {{.tag = 1, .value = 41}, {.tag = 2, .value = 7}};
    increment(&samples[0].value);
    print(samples[0].value);
    print(sizeof(Sample));
    print(alignof(Sample));

    Point point = {.x = 40, .y = 1};
    unsafe { point = translate(point, 1); }
    print(point.x + point.y);
    return 0;
}
