use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_CASE: AtomicUsize = AtomicUsize::new(0);

fn source_file(source: &str) -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-inputs");
    fs::create_dir_all(&directory).unwrap();
    let id = NEXT_CASE.fetch_add(1, Ordering::Relaxed);
    // Unique filenames keep parallel tests from sharing a generated executable.
    let path = directory.join(format!("case_{}_{}.cr", std::process::id(), id));
    fs::write(&path, source).unwrap();
    return path;
}

fn invoke(mode: &str, path: &PathBuf) -> Output {
    return Command::new(env!("CARGO_BIN_EXE_crust"))
        .arg(mode)
        .arg(path)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("compiler executable should run");
}

fn run(source: &str) -> Output {
    return invoke("run", &source_file(source));
}

fn stdout(output: &Output) -> String {
    return String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
}

fn assert_success(output: &Output, expected: &str) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(stdout(output), expected);
}

#[test]
fn compiles_loops_recursion_forward_calls_and_nested_scopes() {
    let output = run(r#"
        int main(void) {
            int n = 5;
            int sum = 0;
            while (n > 0) {
                sum = sum + n;
                n = n - 1;
            }
            print(factorial(5));
            print(sum);
            { int sum = 42; print(sum); }
            print(sum);
            return 0;
        }
        int factorial(int n) {
            if (n <= 1) { return 1; }
            else { return n * factorial(n - 1); }
        }
    "#);
    assert_success(&output, "120\n15\n42\n15\n");
}

#[test]
fn preserves_precedence_associativity_and_integer_semantics() {
    let output = run(r#"
        int main(void) {
            print(1 + 2 * 3);
            print((1 + 2) * 3);
            print(20 / 5 / 2);
            print(20 - 5 - 2);
            print(-7 / 3);
            print(-7 % 3);
            print(!0);
            print(5 & 3 | 8 ^ 1);
            print(1 < 2 == 1);
            print(-2147483648);
            return 0;
        }
    "#);
    assert_success(&output, "7\n9\n2\n13\n-2\n-1\n1\n9\n1\n-2147483648\n");
}

#[test]
fn evaluates_arguments_left_to_right_and_short_circuits_logical_operators() {
    let output = run(r#"
        int echo(int value) { print(value); return value; }
        int pair(int a, int b) { return a * 10 + b; }
        int main(void) {
            print(pair(echo(1), echo(2)));
            print(echo(3) + echo(4));
            print(0 && echo(5));
            print(1 || echo(6));
            print(1 && echo(7));
            print(0 || echo(8));
            return 0;
        }
    "#);
    assert_success(&output, "1\n2\n12\n3\n4\n7\n0\n1\n7\n1\n8\n1\n");
}

#[test]
fn void_calls_and_program_exit_codes_work() {
    let output =
        run("void say(int value) { print(value); return; } int main(void) { say(42); return 7; }");
    assert_eq!(
        output.status.code(),
        Some(7),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(stdout(&output), "42\n");
}

#[test]
fn arithmetic_errors_fail_predictably_instead_of_using_c_undefined_behavior() {
    for (expression, message) in [
        ("2147483647 + 1", "integer overflow"),
        ("-2147483648 - 1", "integer overflow"),
        ("50000 * 50000", "integer overflow"),
        ("-(-2147483648)", "integer overflow"),
        ("1 / 0", "division by zero"),
        ("1 % 0", "remainder by zero"),
        ("-2147483648 / -1", "integer overflow"),
        ("-2147483648 % -1", "integer overflow"),
    ] {
        let output = run(&format!(
            "int main(void) {{\n print({expression});\n return 0;\n}}"
        ));
        assert_eq!(output.status.code(), Some(1));
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(message), "{expression}: {error}");
        assert!(error.contains(".cr:2:8: runtime error:"), "{error}");
    }
}

#[test]
fn cli_keeps_frontend_errors_separate_from_c_compilation() {
    let path = source_file("int main(void) {\n return unknown;\n}");
    let output = invoke("check", &path);
    assert_eq!(output.status.code(), Some(1));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains(":2:9: unknown variable 'unknown'"),
        "{error}"
    );
    assert!(!error.contains("C compiler"), "{error}");
}

#[test]
fn cli_exposes_tokens_check_and_c_output() {
    let path = source_file("int main(void) { return 0; }");
    let checked = invoke("check", &path);
    assert!(checked.status.success());
    assert!(stdout(&checked).contains(": OK"));
    let tokens = invoke("tokens", &path);
    assert!(tokens.status.success());
    assert!(stdout(&tokens).contains("EndOfFile"));
    let c = invoke("emit-c", &path);
    assert!(c.status.success());
    assert!(stdout(&c).contains("int main(void)"));
    assert!(stdout(&c).contains("#include <stdint.h>"));
}
