mod common;

#[test]
fn renamed_imports_and_function_addresses_use_the_native_symbol() {
    let output = common::tracked_run(include_str!("../examples/renamed_imports.cr"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n7\n3\n"
    );
}

#[test]
fn renamed_variadic_imports_preserve_their_argument_protocol_and_callback_type() {
    let output = common::tracked_run(
        r#"
        extern int format(char* destination, size_t capacity, const char* text, ...) = "snprintf";
        typedef int (*Formatter)(char*, size_t, const char*, ...);
        int main() {
            char text[3] = "%d"; char output[16] = ""; Formatter call = &format;
            unsafe { print(call(&output[0], sizeof(output), &text[0], (unsigned short)65535)); }
            print(output); return 0;
        }
    "#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "5\n65535\n"
    );
}

#[test]
fn renamed_imports_require_unsafe_and_conflicting_native_signatures_are_rejected() {
    for source in [
        "extern int absolute(int value) = \"abs\"; int main() { return absolute(-1); }",
        "extern int first(int) = \"native\"; extern char second(int) = \"native\"; int main() { return 0; }",
        "extern int first(int) = \"native\"; extern int second(char) = \"native\"; int main() { return 0; }",
        "extern int first(int) = \"native\"; extern int second(int, ...) = \"native\"; int main() { return 0; }",
        "extern int first(int) = \"one\"; extern int first(int) = \"two\"; int main() { return 0; }",
        "interface I { static int create() = \"native\"; }; int main() { return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
    for symbol in [
        "",
        "0start",
        "int",
        "true",
        "NULL",
        "a-b",
        "a b",
        "symbol();",
        "🦀",
        "a\\0b",
    ] {
        let source = format!("extern int function() = \"{symbol}\"; int main() {{ return 0; }}");
        assert!(
            crust::check(&source).is_err(),
            "should reject symbol {symbol}"
        );
    }
}
