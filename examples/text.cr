import "../lib/string.cr";

int main() {
    char greeting[14] = "Hello, Crust!";
    Result<String, TextError> copied = String::try_from_c_string(&greeting);
    if (!copied.is_ok()) {
        print("Could not create greeting");
        return 1;
    }
    String text = move(copied).take_value();
    print(text.view());

    Result<String, TextError> selected = text.try_substring(7, 5);
    if (!selected.is_ok()) { return 1; }
    String name = move(selected).take_value();
    print(name.view());
    return 0;
}
