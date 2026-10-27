mod common;

#[cfg(not(windows))]
#[test]
fn dllimport_requires_a_windows_target() {
    let source = "extern __declspec(dllimport) int value; int main() { return 0; }";
    assert!(
        crust::check(source)
            .unwrap_err()
            .message
            .contains("Windows target")
    );
}

#[test]
fn native_variables_support_reads_writes_addresses_const_objects_and_callbacks() {
    let source = r#"
        typedef int (*Callback)(int);
        extern int counter;
        extern int other = "counter";
        extern const int limit;
        extern int* pointer;
        extern int* const fixed;
        extern Callback callback;
        int main() {
            print(sizeof(counter) == sizeof(int));
            unsafe {
                print(counter); counter = 5; other += 2;
                int* address = &counter; *address <<= 1;
                print(counter); print(limit); print(*pointer);
                *fixed = 9; print(counter); print(callback(counter));
                const int* bound = &limit; print(*bound);
            }
            { int counter = 42; print(counter); }
            return 0;
        }
    "#;
    let mut generated = crust::compile(source).unwrap();
    generated.push_str(
        r#"
        int counter = 1;
        const int limit = 100;
        int *pointer = &counter;
        int *const fixed = &counter;
        static int increment(int n) { return n + 1; }
        int (*callback)(int) = increment;
    "#,
    );
    let output = common::tracked_generated_c(&generated, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "1\n1\n14\n100\n14\n9\n10\n100\n42\n"
    );
}

#[test]
fn external_access_requires_unsafe_and_const_objects_keep_address_permissions() {
    for body in [
        "print(value);",
        "value = 1;",
        "value += 1;",
        "int* pointer = &value;",
    ] {
        let source = format!("extern int value; int main() {{ {body} return 0; }}");
        assert!(
            crust::check(&source)
                .unwrap_err()
                .message
                .contains("unsafe"),
            "{source}"
        );
    }
    for body in [
        "value = 1;",
        "value += 1;",
        "int* pointer = &value;",
        "int^ borrow = ^value;",
    ] {
        let source =
            format!("extern const int value; int main() {{ unsafe {{ {body} }} return 0; }}");
        assert!(crust::check(&source).is_err(), "{source}");
    }
    crust::check("extern int value; int main() { return sizeof(value) == sizeof(int); }").unwrap();
}

#[test]
fn aliases_of_a_native_object_share_the_borrow_root() {
    let source = "extern int first = \"native\"; extern int second = \"native\"; int main() { unsafe { const int^ view = ^const first; second = 2; print(*view); } return 0; }";
    assert!(
        crust::check(source)
            .unwrap_err()
            .message
            .contains("conflicting access")
    );
}

#[test]
fn external_declarations_validate_types_names_and_native_symbol_consistency() {
    for source in [
        "extern BoxInt owner; int main() { return 0; }",
        "extern int^ borrow; int main() { return 0; }",
        "extern void value; int main() { return 0; }",
        "extern int values[2]; int main() { return 0; }",
        "extern int value; extern const int value; int main() { return 0; }",
        "extern int first = \"native\"; extern char second = \"native\"; int main() { return 0; }",
        "extern int first = \"native\"; extern int call() = \"native\"; int main() { return 0; }",
        "extern int main; int main() { return 0; }",
        "extern int value; int value() { return 0; } int main() { return 0; }",
        "extern int value; T value<T>(T n) { return n; } int main() { return 0; }",
        "extern unsafe int value; int main() { return 0; }",
        "extern int __cdecl value; int main() { return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "{source}");
    }
}

#[test]
fn external_variable_redeclarations_and_uses_have_distinct_source_bindings() {
    let source = "extern int value; extern int value; extern int alias = \"value\"; int main() { unsafe { value = 1; print(alias); } return 0; }";
    let analysis = crust::analyze(source).unwrap();
    let offset = source.find("value = 1").unwrap();
    assert_eq!(analysis.references_at(0, offset, true).len(), 3);
    assert_eq!(analysis.references_at(0, offset, false).len(), 1);
    assert_eq!(
        analysis.definitions_at(0, offset)[0].kind,
        crust::analysis::SymbolKind::Global
    );
    assert!(analysis.rename_target(0, offset).is_some());
    let edits = analysis.rename(0, offset, "renamed").unwrap();
    assert_eq!(edits.len(), 5);
    assert_eq!(
        edits
            .iter()
            .filter(|edit| edit.new_text == "renamed")
            .count(),
        3
    );
    assert_eq!(
        edits
            .iter()
            .filter(|edit| edit.new_text == " = \"value\"")
            .count(),
        2
    );
}
