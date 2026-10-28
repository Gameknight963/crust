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
fn runs_the_method_example_and_frees_the_consumed_receiver() {
    common::assert_example("examples/methods.cr", "42\n7\n7\n", "drop:7\n", false);
}

#[test]
fn supports_forward_method_calls_recursion_self_types_and_shared_reborrows() {
    run(
        r#"
        struct Counter {
            int value;
            void increment() { this.add(1); }
            void add(int n) {
                if (n > 0) {
                    this.value = this.value + 1;
                    this.add(n - 1);
                }
            }
            int get() const { return this.value; }
            int sum(const Self& other) const { return this.get() + other.get(); }
            Self snapshot() const { return this; }
            int finish(Self this) { return this.value; }
        };
        int inspect(const struct Counter& c) { return c.sum(&const c); }
        void update(struct Counter& c) { c.increment(); }
        int main() {
            struct Counter c = { .value = 40 };
            update(&c);
            c.increment();
            print(inspect(&const c));
            struct Counter copy = c.snapshot();
            print(copy.finish());
            print(copy.get());
            return 0;
        }
    "#,
        "84\n42\n42\n",
        "",
    );
}

#[test]
fn consuming_methods_transfer_whole_receivers_through_chains_and_returns() {
    run(
        r#"
        struct Owner {
            Box<int> value;
            Self forward(Self this) { return move(this); }
            int finish(Self this) { return this.value.get(); }
        };
        struct Owner make(int n) { return (struct Owner){ .value = Box<int>::create(n) }; }
        int main() {
            struct Owner a = make(7);
            print(move(a).forward().finish());
            print(make(8).finish());
            make(9).forward();
            print(((struct Owner){ .value = Box<int>::create(10) }).finish());
            return 0;
        }
    "#,
        "7\n8\n10\n",
        "drop:7\ndrop:8\ndrop:9\ndrop:10\n",
    );
}

#[test]
fn evaluates_receivers_before_arguments_and_cleans_up_consuming_parameters_on_return() {
    run(
        r#"
        struct Resource {
            Box<int> value;
            int finish(Self this, Box<int> extra, int early) {
                print(extra.get());
                if (early) { return this.value.get(); }
                return 0;
            }
        };
        struct Resource make(int n) {
            print(n);
            return (struct Resource){ .value = Box<int>::create(n) };
        }
        int mark(int n) { print(n); return n; }
        int main() {
            print(make(11).finish(Box<int>::create(mark(12)), mark(1)));
            print(make(21).finish(Box<int>::create(mark(22)), mark(0)));
            return 0;
        }
    "#,
        "11\n12\n1\n12\n11\n21\n22\n0\n22\n0\n",
        "drop:12\ndrop:11\ndrop:22\ndrop:21\n",
    );
}

#[test]
fn methods_borrow_fields_and_replace_owned_fields_without_dropping_the_receiver() {
    run(
        r#"
        struct Owner {
            Box<int> value;
            void replace(int n) { this.value = Box<int>::create(n); }
            int get() const { return this.value.get(); }
        };
        struct Outer { struct Owner inner; };
        int main() {
            struct Outer o = { .inner = { .value = Box<int>::create(1) } };
            o.inner.replace(2);
            print(o.inner.get());
            return 0;
        }
    "#,
        "2\n",
        "drop:1\ndrop:2\n",
    );
}

#[test]
fn method_names_are_separate_from_functions_builtins_and_other_structs() {
    run(
        r#"
        struct A { int n; int main() const { return this.n; } int print() const { return 2; } };
        struct B { int n; int main() const { return 3; } };
        int main() {
            struct A a = { .n = 1 };
            struct B b = { .n = 0 };
            print(a.main()); print(a.print()); print(b.main());
            return 0;
        }
    "#,
        "1\n2\n3\n",
        "",
    );
}

#[test]
fn rejects_mutation_replacement_and_mutable_reborrows_through_const_this() {
    for method in [
        "void bad() const { this.n = 1; }",
        "void bad() const { this.set(1); }",
        "void bad() const { set_value(&this.n); }",
        "void bad() const { this.owner = Box<int>::create(1); }",
        "void bad() const { this.owner.get_mut() = 1; }",
    ] {
        let source = format!(
            r#"
            void set_value(int& n) {{ n = 1; }}
            struct A {{ int n; Box<int> owner; void set(int n) {{ this.n = n; }} {method} }};
            int main() {{ return 0; }}
        "#
        );
        let error = crust::check(&source).unwrap_err();
        assert!(error.message.contains("shared borrow"), "{source}: {error}");
    }
}

#[test]
fn receiver_loans_cover_later_arguments_and_enclosing_calls() {
    let prelude = r#"
        struct A {
            int n;
            int get() const { return this.n; }
            int set(int n) { this.n = n; return n; }
            int sum(const Self& other) const { return this.n + other.n; }
        };
        int both(int a, int b) { return a + b; }
        int inspect(const struct A& a, int b) { return a.n + b; }
    "#;
    for body in [
        "a.set(a.get());",
        "a.set(a.n);",
        "inspect(&const a, a.set(2));",
    ] {
        let error = crust::check(&format!(
            "{prelude} int main() {{ struct A a = {{ .n = 1 }}; {body} return 0; }}"
        ))
        .unwrap_err();
        assert!(
            error.message.contains("conflicting access") || error.message.contains("expected an"),
            "{body}: {error}"
        );
    }
    crust::check(&format!("{prelude} int main() {{ struct A a = {{ .n = 1 }}; a.sum(&a); a.sum(&const a); both(a.set(2), a.get()); a.set(3); return 0; }}")).unwrap();
}

#[test]
fn owning_receivers_require_moves_and_cannot_be_used_after_consumption() {
    let prelude = "struct A { Box<int> n; int finish(Self this) { return this.n.get(); } };";
    for body in [
        "a.finish();",
        "move(a).finish(); print(a.n.get());",
        "move(a).finish(); move(a).finish();",
    ] {
        let error = crust::check(&format!(
            "{prelude} int main() {{ struct A a = {{ .n = Box<int>::create(1) }}; {body} return 0; }}"
        ))
        .unwrap_err();
        assert!(
            error.message.contains("cannot be copied") || error.message.contains("has been moved"),
            "{body}: {error}"
        );
    }
}

#[test]
fn rejects_invalid_declarations_receivers_and_method_lookup() {
    for source in [
        "struct A { int n; int n() const { return 1; } }; int main() { return 0; }",
        "struct A { int n; int get() { return 1; } int get() const { return 2; } }; int main() { return 0; }",
        "int main() const { return 0; }",
        "struct A { int n; int f(Self this) const { return 0; } }; int main() { return 0; }",
        "struct A { int n; int f(int this) { return 0; } }; int main() { return 0; }",
        "struct A { int n; int f(int x, Self this) { return 0; } }; int main() { return 0; }",
        "struct A { int n; void f() { int this = 1; } }; int main() { return 0; }",
        "int f(int this) { return this; } int main() { return 0; }",
        "int main() { int n = 1; n.missing(); return 0; }",
        "struct A { int n; }; int main() { struct A a = { .n = 1 }; a.missing(); return 0; }",
        "struct A { int n; int f() const { return 1; } }; int main() { struct A a = { .n = 1 }; a.f(2); return 0; }",
        "struct A { int n; int f() const { return 1; } }; struct A make() { return (struct A){ .n = 1 }; } int main() { return make().f(); }",
        "Self f() { return 0; } int main() { return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}
