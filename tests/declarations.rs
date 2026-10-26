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
fn bare_struct_names_work_before_definitions_in_fields_functions_and_borrows() {
    run(
        r#"
        Item make(int n) { return (Item){ .value = n }; }
        struct Parent { Item item; };
        int read(const Item^ item) { return item->value; }
        struct Item { int value; };
        int main() {
            Parent p = { .item = { .value = 41 } };
            struct Item other = make(42);
            print(read(^const p.item));
            print(other.value);
            return 0;
        }
    "#,
        "41\n42\n",
        "",
    );
}

#[test]
fn forward_and_transitive_typedefs_resolve_to_the_underlying_types() {
    run(
        r#"
        typedef Later Count;
        typedef int Later;
        typedef Count Number;
        typedef struct Item Item;
        typedef Item Alias;
        typedef const Number^ ReadRef;
        typedef void Nothing;
        struct Item { Count value; };
        Number read(ReadRef value) { return *value; }
        Nothing show(Alias item) { print(item.value); }
        int main() {
            Alias a = (Alias){ .value = 42 };
            show(a);
            print(read(^const a.value));
            return 0;
        }
    "#,
        "42\n42\n",
        "",
    );
}

#[test]
fn aliases_preserve_ownership_destructors_and_whole_value_moves() {
    run(
        r#"
        typedef BoxInt Owned;
        typedef Resource Handle;
        struct Resource { Owned value; ~Resource() { print(*this->value); } };
        Handle relay(Handle h) { return move(h); }
        int main() {
            Handle first = { .value = box_int(7) };
            Handle second = relay(move(first));
            second = move(second);
            return 0;
        }
    "#,
        "7\n",
        "drop:7\n",
    );
    assert!(
        crust::check(
            "typedef BoxInt Owned; int main() { Owned a = box_int(1); Owned b = a; return 0; }"
        )
        .is_err()
    );
}

#[test]
fn type_and_value_names_coexist_and_explicit_struct_tags_disambiguate() {
    run(
        r#"
        struct Item { int value; };
        typedef int Item;
        int main() {
            Item Item = 2;
            Item = Item + 1;
            int other = 1;
            print((Item));
            print(Item ^ 1);
            print(Item ^ other);
            struct Item object = { .value = 42 };
            print(object.value);
            return 0;
        }
    "#,
        "3\n2\n2\n42\n",
        "",
    );
}

#[test]
fn static_factories_calls_through_aliases_and_self_and_recursion_work() {
    run(
        r#"
        typedef Resource Handle;
        struct Resource {
            BoxInt value;
            static Self create(int n) { return (Self){ .value = box_int(Self::count(n)) }; }
            static int count(int n) { if (n == 0) { return 0; } return 1 + Self::count(n - 1); }
            int get() const { return *this->value; }
            int finish(Self this) { return *this.value; }
            ~Resource() { print(100); }
        };
        int main() {
            Resource a = Handle::create(7);
            print(a.get());
            print(Resource::create(8).finish());
            return 0;
        }
    "#,
        "7\n100\n8\n100\n",
        "drop:8\ndrop:7\n",
    );
}

#[test]
fn static_methods_keep_argument_order_and_borrow_rules() {
    run(
        r#"
        struct A {
            int unused;
            static int sum(const int^ a, const int^ b) { return *a + *b; }
            static int value(int a, int b) { return a + b; }
        };
        int mark(int n) { print(n); return n; }
        int main() {
            int n = 21;
            print(A::sum(^const n, ^const n));
            print(A::value(mark(1), mark(2)));
            return 0;
        }
    "#,
        "42\n1\n2\n3\n",
        "",
    );
    let source = "struct A { int n; static void f(int^ a, const int^ b) {} }; int main() { int n = 1; A::f(^n, ^const n); return 0; }";
    assert!(
        crust::check(source)
            .unwrap_err()
            .message
            .contains("conflicting access")
    );
}

#[test]
fn rejects_invalid_aliases_and_unknown_types() {
    for source in [
        "typedef Missing A; int main() { return 0; }",
        "typedef A A; int main() { return 0; }",
        "typedef B A; typedef A B; int main() { return 0; }",
        "typedef int A; typedef char A; int main() { return 0; }",
        "typedef; int main() { return 0; }",
        "typedef A; int main() { return 0; }",
        "typedef int; int main() { return 0; }",
        "typedef int A",
        "typedef void Nothing; int main() { Nothing x = print(1); return 0; }",
        "Unknown f() { return 0; } int main() { return 0; }",
        "typedef int Self; int main() { return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}

#[test]
fn rejects_invalid_static_members_receivers_and_lookup() {
    for source in [
        "struct A { int n; static int f() { return this->n; } }; int main() { return 0; }",
        "struct A { int n; static int f(Self this) { return 0; } }; int main() { return 0; }",
        "struct A { int n; static int f() const { return 0; } }; int main() { return 0; }",
        "struct A { static int n; }; int main() { return 0; }",
        "struct A { int n; static int f() { return 0; } int f() { return 0; } }; int main() { return 0; }",
        "struct A { int n; int f() { return 0; } static int f() { return 0; } }; int main() { return 0; }",
        "struct A { int n; static int n() { return 0; } }; int main() { return 0; }",
        "struct A { int n; int f() { return 0; } }; int main() { return A::f(); }",
        "struct A { int n; static int f() { return 0; } }; int main() { A a = { .n = 1 }; return a.f(); }",
        "typedef int Number; int main() { return Number::f(); }",
        "struct A { int n; static int f(int n) { return n; } }; int main() { return A::f(); }",
        "struct A { int n; }; int main() { return A::missing(); }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}
