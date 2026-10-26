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
    assert!(output.stderr.is_empty());
}

#[test]
fn native_integer_spellings_preserve_c_library_signatures_and_checked_arithmetic() {
    run(
        include_str!("../examples/native_integers.cr"),
        "-128\n65535\n0\n2147483647\n9223372036854775807\n2147483647\n",
    );
    run(
        r#"
        typedef unsigned short int Half;
        T identity<T>(T value) { return value; }
        int main() {
            int long unsigned first = 42;
            short signed int small = -32768;
            unsigned long long widest = 18446744073709551615;
            widest += 1;
            print(identity<unsigned long>(first)); print(identity<signed short>(small)); print(widest);
            print(sizeof(long) == sizeof(unsigned long)); print(sizeof(long long) == 8);
            Half half = 65535; print(half); return 0;
        }
    "#,
        "42\n-32768\n0\n1\n1\n65535\n",
    );
}

#[test]
fn native_integer_function_pointers_use_exact_c_types() {
    run(
        r#"
        extern long labs(long value);
        extern long long llabs(long long value);
        typedef long (*LongFunction)(long);
        typedef long long (*WideFunction)(long long);
        int main() {
            LongFunction narrow = labs; WideFunction wide = llabs;
            unsafe { print(narrow(-2147483647)); print(wide(-9223372036854775807)); }
            return 0;
        }
    "#,
        "2147483647\n9223372036854775807\n",
    );
}

#[test]
fn native_signed_overflow_and_narrowing_report_runtime_errors() {
    for body in [
        "long limit = (long)(~(unsigned long)0 / 2); print(limit + 1);",
        "long long limit = 9223372036854775807; print(limit + 1);",
        "long long value = 32768; short small = (short)value; print(small);",
    ] {
        let output = common::tracked_run(&format!("int main() {{ {body} return 0; }}"));
        assert!(!output.status.success(), "should fail {body}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("runtime error"));
    }
}

#[test]
fn rejects_conflicting_or_repeated_c_integer_specifiers() {
    for ty in [
        "unsigned unsigned",
        "signed unsigned",
        "short long",
        "long long long",
        "int char",
        "long char",
        "short short",
        "int int",
    ] {
        assert!(
            crust::check(&format!("int main() {{ {ty} value = 0; return 0; }}")).is_err(),
            "should reject {ty}"
        );
    }
}

#[test]
fn compatible_typedefs_and_extern_declarations_can_repeat() {
    run(
        r#"
        typedef short Count;
        typedef int16_t Count;
        typedef Count Count;
        typedef long (*Callback)(long);
        typedef long (*Callback)(long value);
        extern long labs(long first);
        extern long labs(long second);
        extern long labs(long);
        int main() { Count count = 7; print(count); Callback call = labs; unsafe { print(call(-42)); } return 0; }
    "#,
        "7\n42\n",
    );
    for source in [
        "extern int f(int value); extern int64_t f(int value);",
        "extern int f(int value); extern int f(char value);",
        "extern int f(int value, ...); extern int f(int value);",
        "extern int f(int value); int f(int value) { return value; }",
        "typedef int Count; typedef char Count;",
        "typedef int (*Callback)(int); typedef int (*Callback)(int, ...);",
    ] {
        assert!(
            crust::check(&format!("{source} int main() {{ return 0; }}")).is_err(),
            "should reject {source}"
        );
    }
}
