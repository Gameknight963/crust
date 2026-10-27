import "file.cr";

int main() {
    File output = File::temporary();
    if (!output.is_open()) { return 1; }
    if (!output.write_byte('C') || !output.write_byte('r') || !output.write_byte(255u)) {
        return 1;
    }
    if (!output.rewind()) { return 1; }

    File input = move(output);
    print(input.read_byte());
    print(input.read_byte());
    print(input.read_byte());
    print(input.read_byte());
    print(input.eof());
    return 0;
}
