mod common;

use common::tracked_run;

fn assert_run(source: &str, expected: &str, drops: &str) {
    let output = tracked_run(source);
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
fn compiles_the_struct_example_and_cleans_up_its_owner() {
    assert_run(
        include_str!("../examples/structs.cr"),
        "1\n42\n2\n43\n",
        "drop:43\n",
    );
}

#[test]
fn copyable_nested_structs_support_forward_layouts_value_parameters_and_returns() {
    assert_run(
        r#"
        struct Outer { struct Point position; int id; };
        struct Point adjust(struct Point point) {
            point.x = point.x + 1;
            return point;
        }
        struct Point snapshot(const struct Point^ point) { return *point; }
        struct Point { int x; int y; };
        int main() {
            struct Outer first = { .id = 7, .position = { .y = 2, .x = 1 } };
            struct Outer second = first;
            second.position.x = 20;
            struct Point third = adjust(second.position);
            struct Point fourth = snapshot(^const third);
            print(first.position.x);
            print(second.position.x);
            print(fourth.x);
            print(third.y);
            print(first.id);
            return 0;
        }
    "#,
        "1\n20\n21\n2\n7\n",
        "",
    );
}

#[test]
fn nested_owning_fields_drop_in_reverse_declaration_order_not_initializer_order() {
    assert_run(
        r#"
        struct Inner { BoxInt first; BoxInt second; };
        struct Outer { BoxInt before; struct Inner inner; BoxInt after; };
        int main() {
            struct Outer owner = {
                .after = box_int(4),
                .inner = { .second = box_int(3), .first = box_int(2) },
                .before = box_int(1)
            };
            { struct Inner nested = { .first = box_int(5), .second = box_int(6) }; }
            return 0;
        }
    "#,
        "",
        "drop:6\ndrop:5\ndrop:4\ndrop:3\ndrop:2\ndrop:1\n",
    );
}

#[test]
fn transfers_whole_structs_through_calls_returns_replacement_and_discard() {
    assert_run(
        r#"
        struct Item { BoxInt value; };
        struct Item make(int n) { return (struct Item){ .value = box_int(n) }; }
        struct Item pass(struct Item item) { return move(item); }
        int take(struct Item item) { return *item.value; }
        int main() {
            struct Item first = make(10);
            struct Item second = pass(move(first));
            second = (struct Item){ .value = box_int(20) };
            second.value = box_int(30);
            make(40);
            print(take(move(second)));
            first = make(50);
            first = move(first);
            return 0;
        }
    "#,
        "30\n",
        "drop:10\ndrop:20\ndrop:40\ndrop:30\ndrop:50\n",
    );
}

#[test]
fn borrows_fields_and_replaces_owned_fields_without_moving_the_parent() {
    assert_run(
        r#"
        struct Item { int id; BoxInt value; };
        void increment(int^ value) { *value = *value + 1; }
        void update(struct Item^ item) {
            increment(^item->id);
            increment(^*item->value);
        }
        int read(const struct Item^ item) { return item->id + *item->value; }
        void replace(BoxInt^ slot) { *slot = box_int(9); }
        int main() {
            struct Item owner = { .id = 1, .value = box_int(2) };
            update(^owner);
            print(read(^const owner));
            replace(^owner.value);
            print(*owner.value);
            return 0;
        }
    "#,
        "5\n9\n",
        "drop:3\ndrop:9\n",
    );
}

#[test]
fn owning_struct_states_survive_branches_loops_and_reinitialization() {
    assert_run(
        r#"
        struct Item { BoxInt value; };
        struct Item make(int n) { return (struct Item){ .value = box_int(n) }; }
        void take(struct Item item) {}
        int main() {
            struct Item owner = make(0);
            int n = 1;
            while (n < 3) {
                take(move(owner));
                owner = make(n);
                n = n + 1;
            }
            if (n == 3) { take(move(owner)); }
            owner = make(42);
            print(*owner.value);
            return 0;
        }
    "#,
        "42\n",
        "drop:0\ndrop:1\ndrop:2\ndrop:42\n",
    );
}

#[test]
fn initializers_evaluate_in_written_order_and_owning_substruct_replacement_is_safe() {
    assert_run(
        r#"
        struct Inner { BoxInt value; };
        struct Outer { int id; struct Inner inner; };
        int echo(int n) { print(n); return n; }
        int main() {
            struct Outer owner = {
                .inner = { .value = box_int(echo(1)) },
                .id = echo(2)
            };
            owner.inner = (struct Inner){ .value = box_int(3) };
            print(*owner.inner.value);
            return 0;
        }
    "#,
        "1\n2\n3\n",
        "drop:1\ndrop:3\n",
    );
}

#[test]
fn rejects_invalid_layouts_and_initializers_before_generating_c() {
    let cases = [
        (
            "struct Missing make() { return (struct Missing){ .x = 1 }; } int main() { return 0; }",
            "unknown struct 'Missing'",
        ),
        (
            "struct Empty {}; int main() { return 0; }",
            "at least one field",
        ),
        (
            "struct S { int x; int x; }; int main() { return 0; }",
            "already declared",
        ),
        (
            "struct S { int x; }; struct S { int y; }; int main() { return 0; }",
            "already defined",
        ),
        (
            "struct S { void x; }; int main() { return 0; }",
            "void and borrowed fields",
        ),
        (
            "struct S { int^ x; }; int main() { return 0; }",
            "void and borrowed fields",
        ),
        (
            "struct S { struct S child; }; int main() { return 0; }",
            "recursive by-value",
        ),
        (
            "struct A { struct B b; }; struct B { struct A a; }; int main() { return 0; }",
            "recursive by-value",
        ),
        (
            "struct S { int x; }; int main() { struct S s = {}; return 0; }",
            "missing initializer",
        ),
        (
            "struct S { int x; }; int main() { struct S s = { .x = 1, .x = 2 }; return 0; }",
            "more than once",
        ),
        (
            "struct S { int x; }; int main() { struct S s = { .y = 1 }; return 0; }",
            "has no field 'y'",
        ),
        (
            "struct S { int x; }; int main() { struct S s = { .x = box_int(1) }; return 0; }",
            "found BoxInt",
        ),
        (
            "struct A { int x; }; struct B { int x; }; int main() { struct A a = { .x = 1 }; struct B b = a; return 0; }",
            "found struct A",
        ),
    ];
    for (source, message) in cases {
        let error = crust::compile(source).unwrap_err();
        assert!(error.message.contains(message), "{source}: {error}");
    }
}

#[test]
fn rejects_struct_copies_partial_moves_and_access_after_whole_value_moves() {
    let prefix = r#"
        struct Item { int id; BoxInt value; };
        int take(struct Item item) { return *item.value; }
    "#;
    let initial = "struct Item first = { .id = 1, .value = box_int(42) };";
    for (body, message) in [
        ("struct Item second = first;", "cannot be copied"),
        ("take(first);", "cannot be copied"),
        (
            "BoxInt value = first.value;",
            "owning field cannot be copied",
        ),
        ("BoxInt value = move(first.value);", "individual fields"),
        (
            "struct Item second = move(first); print(first.id);",
            "has been moved",
        ),
        (
            "if (1) { take(move(first)); } print(first.id);",
            "may have been moved",
        ),
        (
            "while (1) { take(move(first)); }",
            "restore moved outer owners",
        ),
        ("first.id = take(move(first));", "has been moved"),
        ("first.id = first;", "cannot be copied"),
        ("first.id = first.missing;", "has no field"),
    ] {
        let error = crust::compile(&format!(
            "{prefix} int main() {{ {initial} {body} return 0; }}"
        ))
        .unwrap_err();
        assert!(error.message.contains(message), "{body}: {error}");
    }
}

#[test]
fn rejects_conflicting_field_loans_and_mutation_through_shared_struct_borrows() {
    let prefix = r#"
        struct Item { int id; BoxInt value; };
        void take(const int^ id, struct Item item) {}
        void set(int^ value, int next) { *value = next; }
    "#;
    for (definition, body, message) in [
        (
            "",
            "take(^const first.id, move(first));",
            "conflicting access",
        ),
        ("", "set(^first.id, first.id);", "conflicting access"),
        (
            "void bad(const struct Item^ item) { item->id = 1; }",
            "",
            "shared borrow",
        ),
        (
            "void bad(const struct Item^ item) { *item->value = 1; }",
            "",
            "shared borrow",
        ),
        (
            "void bad(const struct Item^ item) { item->value = box_int(1); }",
            "",
            "shared borrow",
        ),
        (
            "void bad(const struct Item^ item) { set(^item->id, 2); }",
            "",
            "mutable borrow through a shared borrow",
        ),
    ] {
        let source = format!(
            "{prefix} {definition} int main() {{ struct Item first = {{ .id = 1, .value = box_int(42) }}; {body} return 0; }}"
        );
        let error = crust::compile(&source).unwrap_err();
        assert!(error.message.contains(message), "{source}: {error}");
    }
}
