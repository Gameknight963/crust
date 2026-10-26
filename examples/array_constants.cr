typedef int Row[Limits::Columns];

enum Limits {
    Rows = 1 << 1,
    Columns = Rows + 1,
};

int main() {
    Row grid[Limits::Rows] = { { 1, 2, 3 }, { 4, 5, 6 } };
    print(grid[1][2]);
    print(sizeof(grid));
    return 0;
}
