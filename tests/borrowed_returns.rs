mod common;

fn run(source: &str, stdout: &str, drops: &str) {
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
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        drops
    );
}

#[test]
fn borrowed_method_returns_keep_the_receiver_alive() {
    run(
        include_str!("../examples/borrowed_returns.cr"),
        "42\n",
        "drop:42\n",
    );
}

#[test]
fn forwards_returns_through_nested_calls_local_aliases_and_reborrows() {
    run(
        r#"
        int^ identity(int^ value) { return value; }
        const int^ view(int^ value) { return ^const *value; }
        int^ relay(int^ value) {
            int^ local = identity(value);
            return identity(local);
        }
        int main() {
            int n = 1;
            {
                int^ root = ^n;
                {
                    int^ value = identity(^*relay(root));
                    *value += 1;
                }
                *identity(root) += 1;
                print(*view(root));
            }
            print(n);
            return 0;
        }
    "#,
        "3\n3\n",
        "",
    );
}

#[test]
fn borrowed_arrays_fields_and_method_chains_preserve_origins() {
    run(
        r#"
        typedef int Row[2];
        struct Counter {
            int value;
            int read() const { return this->value; }
            Self^ self_mut() { return this; }
        };
        struct Pair {
            Counter counter;
            Row values;
            Counter^ counter_mut() { return ^this->counter; }
            const Row^ row() const { return ^const this->values; }
        };
        int^ element(Row^ row, int index) { return ^(*row)[index]; }
        int main() {
            Pair pair = { .counter = { .value = 3 }, .values = { 4, 5 } };
            pair.counter_mut()->self_mut()->value += 1;
            print(pair.counter_mut()->read());
            print((*pair.row())[1]);
            {
                int^ value = element(^pair.values, 0);
                *value += 2;
            }
            print(pair.values[0]);
            return 0;
        }
    "#,
        "4\n5\n6\n",
        "",
    );
}

#[test]
fn temporary_returned_loans_end_after_statements_conditions_and_calls() {
    run(
        r#"
        int^ identity(int^ value) { return value; }
        const int^ view(const int^ value) { return value; }
        int read(const int^ value) { return *value; }
        int main() {
            int n = 1;
            int copy = *identity(^n);
            n += 1;
            if (*identity(^n)) { n += 1; }
            while (*identity(^n) < 4) { n += 1; }
            print(read(view(^const n)));
            n += 1;
            print(n); print(copy);
            print(sizeof(*identity(^n)));
            (void)identity(^n);
            n += 1; print(n);
            return 0;
        }
    "#,
        "4\n5\n1\n4\n6\n",
        "",
    );
}

#[test]
fn returning_borrows_does_not_transfer_heap_ownership() {
    run(
        r#"
        BoxInt^ slot(BoxInt^ owner) { return owner; }
        const int^ contents(const BoxInt^ owner) { return ^const **owner; }
        int main() {
            BoxInt owner = box_int(1);
            *slot(^owner) = box_int(2);
            print(*contents(^const owner));
            {
                const int^ value = contents(^const owner);
                print(*value);
            }
            BoxInt transferred = move(owner);
            print(*transferred);
            return 0;
        }
    "#,
        "2\n2\n2\n",
        "drop:1\ndrop:2\n",
    );
}

#[test]
fn returned_references_cannot_outlive_or_conflict_with_their_sources() {
    let prelude = "int^ identity(int^ value) { return value; } const int^ view(const int^ value) { return value; }";
    for body in [
        "int n = 1; int^ r = identity(^n); print(n);",
        "int n = 1; const int^ r = view(^const n); n = 2;",
        "int n = 1; int^ r = identity(^n); int^ next = identity(^n);",
        "BoxInt owner = box_int(1); const int^ r = view(^const *owner); BoxInt next = move(owner);",
        "BoxInt owner = box_int(1); const int^ r = view(^const *owner); owner = box_int(2);",
        "int n = 1; int^ root = ^n; int^ child = identity(root); print(*root);",
        "int n = 1; int^ root = ^n; int^ child = ^*identity(root); print(*root);",
        "int n = 1; int^ r = identity(^n); r = identity(^n);",
    ] {
        let source = format!("{prelude} int main() {{ {body} return 0; }}");
        assert!(crust::check(&source).is_err(), "should reject {source}");
    }
}

#[test]
fn rejects_dangling_and_ambiguous_return_contracts() {
    for source in [
        "const int^ bad() { int n = 1; return ^const n; }",
        "const int^ bad(const int^ input) { int n = 1; return ^const n; }",
        "const int^ bad(const int^ input, BoxInt owner) { return ^const *owner; }",
        "const int^ bad(const int^ a, const int^ b) { return a; }",
        "int^ bad(const int^ input) { return ^*input; }",
        "struct Item { int n; const int^ bad(Self this) { return ^const this.n; } };",
        "const int^ bad(const int^ input) { { int n = 1; const int^ r = ^const n; return r; } }",
    ] {
        assert!(
            crust::check(&format!("{source} int main() {{ return 0; }}")).is_err(),
            "should reject {source}"
        );
    }
}
