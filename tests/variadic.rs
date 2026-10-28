mod common;
use common::compiler as crust;

#[test]
fn snprintf_and_variadic_callbacks_promote_narrow_integers_booleans_and_enums() {
    let output = common::tracked_run(include_str!("../examples/variadic.cr"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "7\n255:1:7\n10\n-1:65535:0\n"
    );
}

#[test]
fn variadic_calls_require_fixed_arguments_c_values_and_unsafe_contexts() {
    for body in [
        "consume(0, 1);",
        "unsafe { consume(); }",
        "unsafe { consume(0, Box<int>::create(1)); }",
        "int values[1] = {1}; unsafe { consume(0, values); }",
        "unsafe { consume(0, \"text\"); }",
        "unsafe { consume(0, print(1)); }",
        "typedef void (*Fixed)(int); Fixed callback = consume;",
        "typedef void (*Variadic)(int, ...); Variadic callback = consume; unsafe { callback(); }",
    ] {
        let source =
            format!("extern void consume(int count, ...); int main() {{ {body} return 0; }}");
        assert!(crust::check(&source).is_err(), "should reject {body}");
    }
    for source in [
        "extern int consume(...); int main() { return 0; }",
        "int consume(int count, ...) { return count; } int main() { return 0; }",
        "typedef int (*Callback)(...); int main() { return 0; }",
        "extern int consume(int count, ..., int value); int main() { return 0; }",
        "interface I { static int consume(int count, ...); }; int main() { return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}
