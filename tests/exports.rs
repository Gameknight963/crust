mod common;
use common::compiler as crust;

#[test]
fn library_frontend_omits_main_and_preserves_rename_contracts() {
    let source = "int helper(int value) { return value + 1; } extern int exported(int value) { Box<int> saved = Box<int>::create(value); return helper(saved.get()); }";
    let kind = crust::ProgramKind::Library;
    crust::check_with_kind(source, kind).unwrap();
    assert!(crust::check(source).is_err());
    assert!(crust::check_with_kind("int main() { return 0; }", kind).is_err());
    let analysis = crust::analyze_with_kind(source, kind).unwrap();
    let edits = analysis
        .rename(0, source.find("exported(int").unwrap(), "renamed")
        .unwrap();
    let mut updated = source.to_owned();
    for edit in edits.iter().rev() {
        updated.replace_range(
            edit.location.span.start..edit.location.span.end,
            &edit.new_text,
        );
    }
    let generated = crust::compile_with_kind(&updated, kind).unwrap();
    let c = format!(
        "{generated}\nint main(void) {{ printf(\"%d\\n\", (int)exported(41)); return 0; }}"
    );
    let output = common::tracked_generated_c(&c, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");
    assert_eq!(String::from_utf8_lossy(&output.stderr).trim(), "drop:41");
}

#[test]
fn native_names_prototypes_aliases_recursion_and_callbacks_reach_defined_exports() {
    let source = r#"
        typedef int (*Callback)(int);
        extern int increment(int);
        extern int renamed(int value) = "native_double" { return value * 2; }
        extern int increment(int value) {
            Box<int> local = Box<int>::create(value);
            return local.get() + 1;
        }
        extern int increment(int value);
        extern int alternate(int) = "increment";
        extern int factorial(int value) {
            if (value < 2) { return 1; }
            return value * factorial(value - 1);
        }
        extern int native_call(int);
        int main() {
            Callback callback = increment;
            unsafe {
                print(native_call(20));
                print(alternate(6));
                print(callback(8));
                print(factorial(5));
            }
            return 0;
        }
    "#;
    let c = format!(
        "{}\nint32_t native_call(int32_t value) {{ return increment(native_double(value)); }}",
        crust::compile(source).unwrap()
    );
    let output = common::tracked_generated_c(&c, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "41\n7\n9\n120\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        "drop:40\ndrop:6\ndrop:8\n"
    );
}

#[test]
fn exports_check_bodies_c_abi_contracts_and_native_definition_uniqueness() {
    for declaration in [
        "extern int f(int value) { return missing; }",
        "extern int f(int value) { }",
        "extern int f(int) { return 0; }",
        "extern int f(int value, ...) { return value; }",
        "extern Box<int> f() { return Box<int>::create(1); }",
        "extern int f(int& value) { return value; }",
        "extern int& f(int& value) { return value; }",
        "extern int f(Box<int> value) { return value.get(); }",
        "extern int f() { return 1; } extern int f() { return 2; }",
        "extern int first() = \"native\" { return 1; } extern int second() = \"native\" { return 2; }",
        "extern char f(int); extern int f(int value) { return value; }",
        "extern int f(int value) { return value; } extern int f(char);",
        "extern int f() { return 1; } extern int object = \"f\";",
        "extern int object; extern int f() = \"object\" { return 1; }",
        "extern int f<T>(T value) { return 0; }",
        "extern int entry() = \"main\" { return 0; }",
        "extern int float(int value) { return value; }",
        "extern __declspec(dllimport) int f(int value) { return value; }",
        "extern __declspec(dllimport) int imported(int) = \"f\"; extern int f(int value) { return value; }",
    ] {
        let source = format!("{declaration} int main() {{ return 0; }}");
        assert!(crust::check(&source).is_err(), "{source}");
    }
    assert!(crust::check("extern int f() { return 1; } int main() { return f(); }").is_err());
}

#[test]
fn exported_functions_preserve_native_calling_conventions_and_function_pointer_types() {
    let source = r#"
        typedef int (__stdcall *Callback)(int);
        extern int __stdcall exported(int value) { return value + 1; }
        extern int __stdcall alternate(int) = "exported";
        extern int native_call();
        int main() {
            Callback callback = exported;
            unsafe { print(native_call()); print(callback(6)); print(alternate(8)); }
            return 0;
        }
    "#;
    let c = format!(
        "{}\nint32_t native_call(void) {{ return exported(41); }}",
        crust::compile(source).unwrap()
    );
    let output = common::tracked_generated_c(&c, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n7\n9\n"
    );
}
