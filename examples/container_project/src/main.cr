import "buffer.cr";

struct Point { int x; int y; };

int main() {
    Buffer<int> values = Buffer<int>::create(3, 10);
    *values.at_mut(1) = 20;
    print(*values.at(0));
    print(*values.at(1));

    Buffer<char> letters = Buffer<char>::create(3, 'a');
    *letters.at_mut(1) = 'b';
    print(*letters.at(1));

    Buffer<Point> points = Buffer<Point>::create(2, (Point){ .x = 3, .y = 4 });
    print(points.at(0)->x);
    print(points.at(1)->y);
    return 0;
}
