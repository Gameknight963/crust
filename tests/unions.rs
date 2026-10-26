mod common;

fn run(source: &str, expected: &str) {
    let output = common::tracked_run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        expected
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn union_fields_share_storage_and_whole_values_copy_through_safe_functions() {
    run(
        include_str!("../examples/unions.cr"),
        "42\n18446744073709551615\n1\n",
    );
}

#[test]
fn generic_unions_aliases_arrays_and_methods_use_concrete_layouts() {
    run(
        r#"
        typedef union Number<int> Small;
        union Number<T> {
            T value; uint64_t bits;
            static Self create(T value) { unsafe { return (Self){ .value = value }; } }
            unsafe T read() const { return this->value; }
        };
        int main() {
            Small first = Small::create(42);
            Number<char> byte = Number<char>::create('a');
            Small values[2] = { first, Small::create(7) };
            unsafe { print(values[0].read()); print(values[1].read()); print(byte.read()); }
            print(sizeof(Small) == sizeof(uint64_t)); return 0;
        }
    "#,
        "42\n7\n97\n1\n",
    );
}

#[test]
fn union_fields_and_nested_reborrows_always_overlap_in_the_borrow_checker() {
    let definitions =
        "struct A { int x; int y; }; struct B { int y; int x; }; union U { A a; B b; };";
    for body in [
        "const int^ first = ^const value.a.x; int^ second = ^value.b.y;",
        "const int^ first = ^const value.a.x; value.b.y = 7;",
        "const A^ first = ^const value.a; const int^ inner = ^const first->x; B^ second = ^value.b;",
    ] {
        let source = format!(
            "{definitions} int main() {{ unsafe {{ U value = {{ .a = {{ .x = 1, .y = 2 }} }}; {body} }} return 0; }}"
        );
        let error = crust::check(&source).unwrap_err();
        assert!(error.message.contains("conflicting access"), "{error}");
    }
    run(
        &format!(
            r#"{definitions}
        struct Outer {{ U value; int other; }};
        int main() {{ unsafe {{
            Outer value = {{ .value = {{ .a = {{ .x = 1, .y = 2 }} }}, .other = 3 }};
            int^ first = ^value.value.a.x; int^ second = ^value.other;
            *first = 42; *second = 7; print(*first); print(*second);
        }} return 0; }}
    "#
        ),
        "42\n7\n",
    );
}

#[test]
fn union_destructors_make_the_whole_value_move_only() {
    run(
        r#"
        union Resource {
            int value; uint64_t bits;
            ~Resource() { unsafe { print(this->value); } }
        };
        int main() { unsafe { Resource first = { .value = 42 }; Resource second = move(first); } return 0; }
    "#,
        "42\n",
    );
}

#[test]
fn union_initializers_select_one_field_and_field_operations_require_unsafe() {
    for body in [
        "U value = { .first = 1 };",
        "U value = make(); print(value.first);",
        "U value = make(); value.first = 1;",
        "U value = make(); const int^ field = ^const value.first;",
        "unsafe { U value = {}; }",
        "unsafe { U value = { .first = 1, .second = 2 }; }",
        "unsafe { U value = { .missing = 1 }; }",
    ] {
        let source = format!(
            "union U {{ int first; int second; }}; U make() {{ unsafe {{ return (U){{ .first = 1 }}; }} }} int main() {{ {body} return 0; }}"
        );
        assert!(crust::check(&source).is_err(), "should reject {body}");
    }
    for source in [
        "union U { BoxInt value; }; int main() { return 0; }",
        "struct Owner { BoxInt value; }; union U { Owner value; }; int main() { return 0; }",
        "union U { BoxInt values[2]; }; int main() { return 0; }",
        "union U { int value; }; struct U; int main() { return 0; }",
        "struct U; union U { int value; }; int main() { return 0; }",
        "union U { int value; }; int main() { struct U* value = 0; return 0; }",
        "enum U { Value }; typedef union U* Handle; int main() { return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}
