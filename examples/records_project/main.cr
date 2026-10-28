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
    Readings readings = {.id = point.x + point.y, .updates = 0, .values = {40, 2, 0}};
    increment(&readings.values[0]);
    readings.updates++;
    if (readings.values[0] != 41 || readings.updates != 1) { return 1; }
    print(readings.id);
    return 0;
}
