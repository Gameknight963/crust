mod common;

fn run(source: &str, expected: &str) {
    let output = common::tracked_run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        expected
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn runs_string_example_and_supports_shared_print_borrows() {
    run(
        include_str!("../examples/strings.cr"),
        "Hello, Crust!\nhello, Crust!\nAdjacent strings\n",
    );
}

#[test]
fn literals_are_utf8_byte_arrays_with_zero_termination_and_padding() {
    run(
        r#"
        int main() {
            uint8_t text[5] = "é";
            print(text[0]); print(text[1]); print(text[2]); print(text[4]);
            print(sizeof("é")); print(sizeof("")); print("");
            print(text);
            text[0] = 'A'; text[1] = 0;
            print(text);
            text = "Hi"; print(text);
            print(sizeof("😀")); print("😀");
            return 0;
        }
    "#,
        "195\n169\n0\n0\n3\n1\n\né\nA\nHi\n5\n😀\n",
    );
}

#[test]
fn concatenation_comments_and_escapes_preserve_exact_bytes() {
    run(
        r#"
        int main() {
            print("a" /* between */ "b" // between
                  "c");
            print("\"quoted\" \\ \x41\101\t!");
            print("first\0ignored");
            print("// inside /* string */");
            print(sizeof("\0" "x"));
            return 0;
        }
    "#,
        "abc\n\"quoted\" \\ AA\t!\nfirst\n// inside /* string */\n3\n",
    );
}

#[test]
fn bounded_printing_handles_arrays_without_a_terminating_zero() {
    run(
        r#"
        typedef uint8_t Bytes[3];
        void show(Bytes^ text) { print(text); (*text)[0] = 'X'; }
        int main() {
            Bytes bytes = { 'a', 'b', 'c' };
            print(bytes); print(^const bytes);
            show(^bytes); print(bytes);
            return 0;
        }
    "#,
        "abc\nabc\nabc\nXbc\n",
    );
}

#[test]
fn strings_copy_through_fields_arguments_and_return_values() {
    run(
        r#"
        typedef uint8_t Word[4];
        struct Label { Word text; };
        Word make() { return "cat"; }
        Word change(Word word) { word[0] = 'b'; return word; }
        int main() {
            Label label = { .text = make() };
            print(change(label.text)); print(label.text);
            label.text = "dog"; print(label.text);
            return 0;
        }
    "#,
        "bat\ncat\ndog\n",
    );
}

#[test]
fn malformed_strings_and_incompatible_destinations_report_frontend_errors() {
    for source in [
        "int main() { print(\"unterminated); return 0; }",
        "int main() { print(\"line\nbreak\"); return 0; }",
        r#"int main() { print("\q"); return 0; }"#,
        r#"int main() { print("\x100"); return 0; }"#,
        r#"int main() { print("\400"); return 0; }"#,
        r#"int main() { uint8_t text[2] = "hi"; return 0; }"#,
        r#"int main() { int text[3] = "hi"; return 0; }"#,
        r#"int main() { int8_t text[3] = "hi"; return 0; }"#,
        r#"int main() { bool text[3] = "hi"; return 0; }"#,
        r#"int main() { uint8_t text[3] = { 1, 2, 3 }; print(^text[0]); return 0; }"#,
        r#"int main() { print(^const "hi"); return 0; }"#,
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}
