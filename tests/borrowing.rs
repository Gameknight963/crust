use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn borrows_stack_values_box_contents_and_owner_slots_without_transferring_ownership() {
    let source = r#"
        void increment(int^ value) { *value = *value + 1; }
        void relay(int^ value) { increment(value); increment(^*value); }
        int read(const int^ value) { return *value; }
        int sum(const int^ a, const int^ b) { return *a + *b; }
        void replace(BoxInt^ owner, int value) { *owner = box_int(value); }
        int read_box(const BoxInt^ owner) { return **owner; }
        int main(void) {
            int number = 40;
            relay(^number);
            print(read(^const number));
            print(sum(^const number, ^const number));
            BoxInt owner = box_int(7);
            increment(^*owner);
            print(read_box(^const owner));
            replace(^owner, 99);
            print(*owner);
            print(sum(^const *owner, ^const *owner));
            return 0;
        }
    "#;
    let generated = crust::compile(source).unwrap();
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("borrowing-tests");
    fs::create_dir_all(&directory).unwrap();
    let c_path = directory.join(format!("borrowing_{}.c", std::process::id()));
    let executable = c_path.with_extension(std::env::consts::EXE_EXTENSION);
    fs::write(&c_path, generated).unwrap();
    crust::native::compile_file(&c_path, &executable).unwrap();
    let output = Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n84\n8\n99\n198\n"
    );
}

#[test]
fn rejects_aliases_moves_and_reads_that_conflict_with_live_argument_borrows() {
    let prelude = r#"
        void both(int^ a, int^ b) { *a = *b; }
        void mixed(int^ a, const int^ b) { *a = *b; }
        void value(int^ a, int b) { *a = b; }
        int read(const int^ a) { return *a; }
        void owner_and_borrow(const BoxInt^ a, BoxInt b) { print(**a + *b); }
        void owned_first(BoxInt a, const BoxInt^ b) { print(*a + **b); }
    "#;
    for body in [
        "int n = 1; both(^n, ^n);",
        "int n = 1; mixed(^n, ^const n);",
        "int n = 1; value(^n, n);",
        "int n = 1; value(^n, read(^const n));",
        "BoxInt a = box_int(1); owner_and_borrow(^const a, move(a));",
        "BoxInt a = box_int(1); value(^*a, owned_first(move(a), ^const a));",
        "BoxInt a = box_int(1); owned_first(move(a), ^const a);",
    ] {
        let error = crust::compile(&format!("{prelude} int main(void) {{ {body} return 0; }}"))
            .unwrap_err();
        assert!(
            error.message.contains("conflicting access")
                || error.message.contains("has been moved"),
            "{body}: {error}"
        );
    }
}

#[test]
fn rejects_mutation_through_shared_borrows_and_escaping_references() {
    let cases = [
        ("void bad(const int^ a) { *a = 1; }", "shared borrow"),
        ("void bad(const BoxInt^ a) { **a = 1; }", "shared borrow"),
        (
            "void bad(const BoxInt^ a) { *a = box_int(1); }",
            "shared borrow",
        ),
        (
            "void set(int^ a) { *a = 1; } void bad(const int^ a) { set(^*a); }",
            "mutable borrow through a shared borrow",
        ),
        ("void bad(int^ a) { int^ saved = a; }", "borrowed locals"),
        (
            "const int^ bad(void) { int a = 1; return ^const a; }",
            "lifetime contracts",
        ),
        ("void bad(int^ a) { a = ^*a; }", "cannot be rebound"),
        ("void bad(int^ a) { a; }", "passed directly"),
    ];
    for (definition, message) in cases {
        let error =
            crust::compile(&format!("{definition} int main(void) {{ return 0; }}")).unwrap_err();
        assert!(error.message.contains(message), "{definition}: {error}");
    }
}

#[test]
fn releases_argument_loans_after_calls_and_allows_shared_aliases() {
    crust::check(
        r#"
        int sum(const int^ a, const int^ b) { return *a + *b; }
        void set(int^ a, int value) { *a = value; }
        int read(const int^ a) { return *a; }
        int main(void) {
            int n = 1;
            sum(^const n, ^const n);
            set(^n, 2);
            n = read(^const n) + 1;
            set(^n, 3);
            BoxInt owner = box_int(n);
            print(read(^const *owner));
            BoxInt next = move(owner);
            print(*next);
            return 0;
        }
    "#,
    )
    .unwrap();
}
