mod common;

fn run(source: &str, stdout: &str) {
    let output = common::tracked_run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        stdout
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn runs_enum_example_with_methods_scoped_and_unscoped_names() {
    run(include_str!("../examples/enums.cr"), "1\n4\n");
}

#[test]
fn enums_support_forward_uses_typedefs_arrays_and_value_returns() {
    run(
        r#"
        typedef enum Color Shade;
        Color relay(Shade value) { return value; }
        struct Palette { Color colors[2]; };
        enum Color { Red = -2, Green, Blue = 7, SameBlue = 7, };
        int main() {
            Palette p = { .colors = { Red, Shade::Blue } };
            Color copy = relay(p.colors[0]);
            print((int)copy); print((int)Green);
            print(p.colors[1] == SameBlue);
            int Red = 42; print(Red); print((int)Color::Red);
            print(sizeof(Color)); print(alignof(Shade));
            return 0;
        }
    "#,
        "-2\n-1\n1\n42\n-2\n4\n4\n",
    );
}

#[test]
fn enum_borrows_and_local_references_preserve_type_and_permissions() {
    run(
        r#"
        enum State : uint8_t { Off, On };
        void turn_on(State^ state) { *state = On; }
        int read(const State^ state) { return (int)*state; }
        int main() {
            State state = Off;
            {
                State^ reference = ^state;
                turn_on(reference);
                print(read(^const *reference));
            }
            print((int)state); print(sizeof(State));
            return 0;
        }
    "#,
        "1\n1\n1\n",
    );
}

#[test]
fn explicit_casts_check_representations_and_allow_unnamed_values() {
    run(
        r#"
        enum Byte : uint8_t { Zero, Maximum = 255 };
        enum Wide : uint64_t { Huge = 18446744073709551615 };
        enum Signed : int64_t { Minimum = -9223372036854775808 };
        int main() {
            Byte byte = (Byte)42;
            print((int)byte); print((bool)byte); print((bool)Zero);
            print((uint64_t)Huge); print((int64_t)Minimum);
            byte = (Byte)(int)Maximum; print((int)byte);
            print(Minimum < (Signed)0);
            return 0;
        }
    "#,
        "42\n1\n0\n18446744073709551615\n-9223372036854775808\n255\n1\n",
    );
    for source in [
        "enum Byte : uint8_t { Zero }; int main() { int n = -1; Byte b = (Byte)n; return 0; }",
        "enum Byte : uint8_t { Zero }; int main() { int n = 256; Byte b = (Byte)n; return 0; }",
        "enum Big : uint64_t { Maximum = 18446744073709551615 }; int main() { print((int64_t)Maximum); return 0; }",
    ] {
        let output = common::tracked_run(source);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("integer conversion out of range")
        );
    }
}

#[test]
fn rejects_invalid_definitions_values_and_name_collisions() {
    for source in [
        "enum Empty {}; int main() { return 0; }",
        "enum E { A }; enum E { B }; int main() { return 0; }",
        "enum E { A, A }; int main() { return 0; }",
        "enum E { A }; enum F { A }; int main() { return 0; }",
        "enum E { print }; int main() { return 0; }",
        "enum E { this }; int main() { return 0; }",
        "enum E { A }; void A() {} int main() { return 0; }",
        "enum E { A }; struct E { int n; }; int main() { return 0; }",
        "enum E : bool { A }; int main() { return 0; }",
        "enum E : uint8_t { A = 256 }; int main() { return 0; }",
        "enum E : uint8_t { A = -1 }; int main() { return 0; }",
        "enum E : uint8_t { A = 255, B }; int main() { return 0; }",
        "enum E { A = 2147483648 }; int main() { return 0; }",
        "enum E { A }; int main() { E a = E::Missing; return 0; }",
        "int main() { enum Missing value = 0; return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}

#[test]
fn enum_arithmetic_and_integer_conversions_require_explicit_casts() {
    let prelude = "enum E { A }; enum F { B }; void set(E^ value) {}";
    for body in [
        "E value = 0;",
        "int value = A;",
        "F value = A;",
        "E value = A; value += 1;",
        "print(A + A);",
        "print(1 + A);",
        "print(A == 0);",
        "print(A == B);",
        "print(A);",
        "if (A) {}",
        "E value = (E)box_int(1);",
        "int n = 0; set(^n);",
    ] {
        let source = format!("{prelude} int main() {{ {body} return 0; }}");
        assert!(crust::check(&source).is_err(), "should reject {source}");
    }
}
