mod common;
use common::compiler as crust;

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
fn local_borrows_reborrow_and_release_at_block_end() {
    common::assert_example(
        "examples/borrowed_locals.cr",
        "41\n42\n",
        "drop:42\n",
        false,
    );
}

#[test]
fn shared_locals_coexist_and_allow_direct_shared_access() {
    run(
        r#"
        int read(const int& value) { return value; }
        int sum(const int& a, const int& b) { return a + b; }
        int main() {
            int n = 7;
            {
                const int& a = &const n;
                const int& b = a;
                print(n); print(a); print(read(b)); print(sum(a, b));
                (void)a;
                print(sizeof(a));
            }
            n = 8; print(n);
            return 0;
        }
    "#,
        &format!("7\n7\n7\n14\n{}\n8\n", std::mem::size_of::<i32>()),
        "",
    );
}

#[test]
fn mutable_locals_forward_reborrow_and_resume_after_inner_bindings() {
    run(
        r#"
        typedef int& Ref;
        void increment(int& value) { value += 1; }
        void relay(int& value) {
            int& local = value;
            increment(local);
            {
                int& inner = local;
                inner += 1;
            }
            increment(&local);
        }
        int main() {
            int n = 1;
            {
                Ref a = &n;
                relay(a); increment(a);
                print(a);
            }
            print(n);
            return 0;
        }
    "#,
        "5\n5\n",
        "",
    );
}

#[test]
fn owner_slot_borrows_replace_values_and_preserve_cleanup() {
    run(
        r#"
        void replace(Box<int>& owner) { owner = Box<int>::create(3); }
        int main() {
            Box<int> owner = Box<int>::create(1);
            {
                Box<int>& slot = &owner;
                slot = Box<int>::create(2);
                replace(slot);
                {
                    const int& value = slot.get();
                    print(value);
                }
                slot.get_mut() += 1;
            }
            Box<int> transferred = move(owner);
            print(transferred.get());
            return 0;
        }
    "#,
        "3\n4\n",
        "drop:1\ndrop:2\ndrop:4\n",
    );
}

#[test]
fn local_struct_and_array_references_support_methods_fields_and_indices() {
    run(
        r#"
        struct Counter {
            int value;
            void increment() { this.value += 1; }
            int read() const { return this.value; }
        };
        typedef uint8_t Text[4];
        int main() {
            Counter counters[2] = { { .value = 1 }, { .value = 2 } };
            {
                Counter& item = &counters[1];
                item.increment(); print(item.read());
            }
            print(counters[1].value);
            Text text = "cat";
            {
                Text& reference = &text;
                (reference)[0] = 'b';
                print(&const reference);
            }
            print(text);
            return 0;
        }
    "#,
        "3\n3\nbat\nbat\n",
        "",
    );
}

#[test]
fn branch_loop_and_shadowing_scopes_release_local_loans() {
    run(
        r#"
        int main() {
            int n = 0;
            for (int i = 0; i < 3; i += 1) {
                int& value = &n;
                value += 1;
                if (i < 2) { continue; }
                break;
            }
            n += 1;
            if (n) { int& value = &n; value += 1; }
            else { int& value = &n; value += 2; }
            {
                int& value = &n;
                { int value = 9; print(value); }
                print(value);
            }
            print(n);
            return 0;
        }
    "#,
        "9\n5\n5\n",
        "",
    );
}

#[test]
fn local_loans_preserve_exclusivity_and_block_owner_invalidation() {
    let prelude = "void pair(int& a, int& b) {} void mixed(int& a, int b) {}";
    for body in [
        "int n = 1; int& a = &n; print(n);",
        "int n = 1; int& a = &n; n = 2;",
        "int n = 1; const int& a = &const n; n += 1;",
        "int n = 1; const int& a = &const n; int& b = &n;",
        "int n = 1; int& a = &n; int& b = &n;",
        "int n = 1; int& a = &n; int& b = a; print(a);",
        "int n = 1; int& a = &n; const int& b = &const a; a = 2;",
        "int n = 1; int& a = &n; pair(a, a);",
        "int n = 1; int& a = &n; mixed(a, a);",
        "Box<int> a = Box<int>::create(1); const int& view = a.get(); Box<int> b = move(a);",
        "Box<int> a = Box<int>::create(1); const int& view = a.get(); a = Box<int>::create(2);",
        "Box<int> a = Box<int>::create(1); Box<int>& slot = &a; const int& view = slot.get(); slot = Box<int>::create(2);",
        "int n = 1; const int& a = &const n; int& b = &a;",
    ] {
        let source = format!("{prelude} int main() {{ {body} return 0; }}");
        assert!(crust::check(&source).is_err(), "should reject {source}");
    }
}

#[test]
fn reference_bindings_cannot_be_rebound_or_escape() {
    for source in [
        "int main() { int a = 1; int b = 2; int& r = &a; r = &b; return 0; }",
        "int main() { int n = 1; int& r = &n; int&& rr = &r; return 0; }",
        "const int& escape() { int n = 1; const int& r = &const n; return r; } int main() { return 0; }",
        "int main() { int& r = r; return 0; }",
        "int main() { int a = 1; { int& r = &a; } print(r); return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}
