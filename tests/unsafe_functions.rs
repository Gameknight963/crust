mod common;

#[test]
fn unsafe_functions_methods_and_generic_instances_require_an_unsafe_caller() {
    let definitions = r#"
        unsafe int read(int* value) { return *value; }
        unsafe T read_generic<T>(T* value) { return *value; }
        struct View {
            int* data;
            unsafe int get() const { return *this.data; }
            static unsafe Self create(int* data) { return (Self){ .data = data }; }
        };
    "#;
    let source = format!(
        "{definitions} int main() {{ int value = 42; unsafe {{ View view = View::create(&value); print(read(&value)); print(read_generic<int>(&value)); print(view.get()); }} return 0; }}"
    );
    let output = common::tracked_run(&source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n42\n42\n"
    );
    for body in [
        "int* pointer = 0; read(pointer);",
        "int* pointer = 0; read_generic<int>(pointer);",
        "int* pointer = 0; View::create(pointer);",
        "View value = { .data = 0 }; value.get();",
    ] {
        let error =
            crust::check(&format!("{definitions} int main() {{ {body} return 0; }}")).unwrap_err();
        assert!(
            error.message.contains("calling an unsafe function"),
            "{error}"
        );
    }
}

#[test]
fn unsafe_bodies_retain_ownership_checks_and_main_requires_a_safe_signature() {
    for source in [
        "unsafe int main() { return 0; }",
        "unsafe void consume() { BoxInt a = box_int(1); BoxInt b = move(a); print(*a); } int main() { return 0; }",
        "unsafe void mutate(const int& value) { value = 1; } int main() { return 0; }",
        "struct S { unsafe int value; }; int main() { return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
    crust::check("unsafe int read(int* value) { return *value; } int main() { print(sizeof(read(0))); return 0; }").unwrap();
}

#[test]
fn interfaces_require_safe_implementations_for_safe_requirements() {
    let source = "interface Readable { int get() const; }; struct V { int x; unsafe int get() const { return this.x; } }; int ignore<T: Readable>() { return 0; } int main() { return ignore<V>(); }";
    assert!(
        crust::check(source)
            .unwrap_err()
            .message
            .contains("does not satisfy")
    );
    crust::check("interface Readable { unsafe int get() const; }; struct V { int x; int get() const { return this.x; } }; int ignore<T: Readable>() { return 0; } int main() { return ignore<V>(); }").unwrap();
    crust::check("static interface Factory { unsafe Self create(); }; struct V { int x; static unsafe Self create() { return (Self){ .x = 1 }; } }; T make<T: Factory>() { unsafe { return T::create(); } } int main() { V value = make<V>(); return value.x; }").unwrap();
}

#[test]
fn unsafe_static_function_addresses_preserve_native_callback_calls() {
    let output = common::tracked_run(
        r#"
        typedef int (*Callback)(int*);
        struct Helpers { int unused; static unsafe int read(int* value) { return *value; } };
        int main() { int value = 42; Callback callback = Helpers::read; unsafe { print(callback(&value)); } return 0; }
    "#,
    );
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n"
    );
}
