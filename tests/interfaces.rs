mod common;
use common::compiler as crust;

fn run(source: &str, expected: &str, drops: &str) {
    let output = common::tracked_run_sizes(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        expected
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        drops
    );
}

#[test]
fn structural_constraints_and_static_interfaces_use_ordinary_structs() {
    run(include_str!("../examples/interfaces.cr"), "42\n", "");
}

#[test]
fn multiple_constraints_and_generic_structs_resolve_self_and_forward_interfaces() {
    run(
        r#"
        struct Wrapper<T: Readable + Factory> {
            T value;
            static Self create(int value) { return (Self){ .value = T::create(value) }; }
            int read() const { return this.value.read(); }
        };
        int result<T: Readable + Factory>(int value) {
            T item = T::create(value); return item.read();
        }
        struct Number { int value;
            static Self create(int value) { return (Self){ .value = value }; }
            int read() const { return this.value; }
        };
        interface Readable { int read() const; };
        static interface Factory { Self create(int value); };
        int main() {
            Wrapper<Number> value = Wrapper<Number>::create(17);
            print(value.read()); print(result<Wrapper<Number>>(42)); return 0;
        }
    "#,
        "17\n42\n",
        "",
    );
}

#[test]
fn interfaces_preserve_value_receivers_borrowed_returns_and_owner_cleanup() {
    run(
        r#"
        interface Resource {
            const int& get() const;
            Box<int> take(Self this);
            static Self create(int value);
        };
        struct Owner {
            Box<int> value;
            const int& get() const { return this.value.get(); }
            Box<int> take(Self this) { Box<int> value = Box<int>::create(this.value.get()); return move(value); }
            static Self create(int value) { return (Self){ .value = Box<int>::create(value) }; }
        };
        Box<int> copy_out<T: Resource>(int value) {
            T owner = T::create(value);
            print(owner.get()); return move(owner).take();
        }
        int main() { Box<int> value = copy_out<Owner>(42); print(value.get()); return 0; }
    "#,
        "42\n42\n",
        "drop:bytes:4\ndrop:bytes:4\n",
    );
}

#[test]
fn constraints_are_checked_even_when_the_generic_body_does_not_call_members() {
    for method in [
        "int other() const { return 1; }",
        "int read() { return 1; }",
        "int64_t read() const { return 1; }",
        "int read(int value) const { return value; }",
        "static int read() { return 1; }",
    ] {
        let source = format!(
            "interface Readable {{ int read() const; }}; struct Value {{ int field; {method} }}; int ignore<T: Readable>() {{ return 0; }} int main() {{ return ignore<Value>(); }}"
        );
        let error = crust::check(&source).unwrap_err();
        assert!(
            error
                .message
                .contains("does not satisfy interface 'Readable'"),
            "{error:?}"
        );
    }
    let error = crust::check("static interface Factory { Self create(); }; struct Value { int field; Self create() { return (Self){ .field = 1 }; } }; int ignore<T: Factory>() { return 0; } int main() { return ignore<Value>(); }").unwrap_err();
    assert!(
        error
            .message
            .contains("does not satisfy interface 'Factory'")
    );
}

#[test]
fn rejects_unknown_interfaces_invalid_requirements_and_nonstruct_arguments() {
    for source in [
        "int ignore<T: Missing>() { return 0; } int main() { return 0; }",
        "interface X {}; interface X {}; int main() { return 0; }",
        "interface X { int read() { return 0; } }; int main() { return 0; }",
        "interface X { Unknown read(); }; int main() { return 0; }",
        "interface X { int read(int x, int x); }; int main() { return 0; }",
        "interface X { int read() const; }; int f<T: X>() { return 0; } int main() { return f<int>(); }",
        "static interface X { int read() const; }; struct V { int x; }; int f<T: X>() { return 0; } int main() { return f<V>(); }",
        "interface X { int read(); int read(); }; struct V { int x; }; int f<T: X>() { return 0; } int main() { return f<V>(); }",
        "interface X { int read() { return 0; } }; struct V { int x; }; int f<T: X>() { return 0; } int main() { return f<V>(); }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}
