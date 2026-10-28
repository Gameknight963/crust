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
fn example_destroys_nested_replaced_and_final_values_but_not_moved_sources() {
    run(
        include_str!("../examples/destructors.cr"),
        "20\n10\n30\n",
        "",
    );
}

#[test]
fn destructor_runs_before_fields_and_nested_fields_drop_in_reverse_order() {
    run(
        r#"
        struct Child {
            int id;
            Box<int> value;
            ~Child() { print(this.id); print(this.value.get()); }
        };
        struct Parent {
            struct Child first;
            struct Child second;
            ~Parent() {
                print(100);
                this.first.id = 11;
                print(this.second.value.get());
            }
        };
        int main() {
            struct Parent parent = {
                .first = { .id = 1, .value = Box<int>::create(21) },
                .second = { .id = 2, .value = Box<int>::create(22) }
            };
            return 0;
        }
    "#,
        "100\n22\n2\n22\n11\n21\n",
        "drop:22\ndrop:21\n",
    );
}

#[test]
fn return_from_destructor_still_cleans_fields_and_destructor_locals() {
    run(
        r#"
        struct A {
            Box<int> value;
            ~A() {
                Box<int> temporary = Box<int>::create(9);
                print(this.value.get());
                return;
            }
        };
        int main() {
            struct A a = { .value = Box<int>::create(7) };
            return 0;
        }
    "#,
        "7\n",
        "drop:9\ndrop:7\n",
    );
}

#[test]
fn whole_moves_returns_consuming_methods_and_discards_destroy_exactly_once() {
    run(
        r#"
        struct A {
            int id;
            ~A() { print(this.id); }
            Self forward(Self this) { return move(this); }
        };
        struct A make(int n) { return (struct A){ .id = n }; }
        struct A relay(struct A a) { return move(a); }
        int main() {
            struct A first = make(1);
            struct A second = relay(move(first));
            second = move(second);
            move(second).forward();
            make(2).forward();
            struct A third = make(3);
            first = make(4);
            return 0;
        }
    "#,
        "1\n2\n3\n4\n",
        "",
    );
}

#[test]
fn moves_of_parents_without_custom_destructors_skip_their_child_destructors() {
    run(
        r#"
        struct Child { int id; ~Child() { print(this.id); } };
        struct Parent { struct Child child; };
        struct Parent relay(struct Parent p) { return move(p); }
        int main() {
            struct Parent a = { .child = { .id = 5 } };
            struct Parent b = relay(move(a));
            return 0;
        }
    "#,
        "5\n",
        "",
    );
}

#[test]
fn replacement_through_fields_borrows_and_methods_drops_old_values() {
    run(
        r#"
        struct Child { int id; ~Child() { print(this.id); } };
        struct Parent {
            struct Child child;
            void replace(int id) { this.child = (struct Child){ .id = id }; }
        };
        void replace(struct Child& slot, int n) { slot = (struct Child){ .id = n }; }
        int main() {
            struct Parent parent = { .child = { .id = 1 } };
            parent.replace(2);
            replace(&parent.child, 3);
            parent.child = (struct Child){ .id = 4 };
            return 0;
        }
    "#,
        "1\n2\n3\n4\n",
        "",
    );
}

#[test]
fn branches_loops_and_early_returns_drop_only_live_instances_in_reverse_order() {
    run(
        r#"
        struct A { int id; ~A() { print(this.id); } };
        void consume(struct A a) { return; }
        void branch(int take) {
            struct A a = { .id = 10 + take };
            if (take) { consume(move(a)); }
            struct A b = { .id = 20 + take };
            return;
        }
        int main() {
            branch(0);
            branch(1);
            int n = 0;
            while (n < 2) {
                struct A a = { .id = 30 + n };
                n = n + 1;
            }
            return 0;
        }
    "#,
        "20\n10\n11\n21\n30\n31\n",
        "",
    );
}

#[test]
fn destructor_has_exclusive_this_and_can_call_methods_and_replace_owning_fields() {
    run(
        r#"
        struct A {
            Box<int> value;
            ~A() { this.replace(8); print(this.get()); }
            int get() const { return this.value.get(); }
            void replace(int n) { this.value = Box<int>::create(n); }
        };
        int main() {
            struct A a = { .value = Box<int>::create(7) };
            return 0;
        }
    "#,
        "8\n",
        "drop:7\ndrop:8\n",
    );
}

#[test]
fn drop_is_an_ordinary_method_name_and_does_not_define_a_destructor() {
    run(
        r#"
        struct A { int id; void drop() { print(this.id); } };
        int main() {
            struct A a = { .id = 7 };
            struct A b = a;
            b.drop();
            return 0;
        }
    "#,
        "7\n",
        "",
    );
}

#[test]
fn custom_destructors_make_plain_fields_and_enclosing_structs_move_only() {
    let prelude = "struct A { int id; ~A() {} }; struct Parent { struct A child; };";
    for body in [
        "struct A a = { .id = 1 }; struct A b = a;",
        "struct A a = { .id = 1 }; move(a); print(a.id);",
        "struct Parent a = { .child = { .id = 1 } }; struct Parent b = a;",
        "struct Parent a = { .child = { .id = 1 } }; struct A b = a.child;",
        "struct A a = { .id = 1 }; if (1) { move(a); } print(a.id);",
        "struct A a = { .id = 1 }; while (0) { move(a); }",
    ] {
        assert!(
            crust::check(&format!("{prelude} int main() {{ {body} return 0; }}")).is_err(),
            "should reject {body}"
        );
    }
}

#[test]
fn rejects_invalid_destructors_and_explicit_calls() {
    for source in [
        "struct A { int id; ~B() {} }; int main() { return 0; }",
        "struct A { int id; ~A() {} ~A() {} }; int main() { return 0; }",
        "struct A { int id; ~A(int n) {} }; int main() { return 0; }",
        "struct A { int id; ~A(Self this) {} }; int main() { return 0; }",
        "struct A { int id; ~A() const {} }; int main() { return 0; }",
        "struct A { int id; void ~A() {} }; int main() { return 0; }",
        "struct A { int id; ~A() { return 1; } }; int main() { return 0; }",
        "struct A { int id; ~A() { move(this); } }; int main() { return 0; }",
        "struct A { int id; ~A() {} }; int main() { struct A a = { .id = 1 }; a.~A(); return 0; }",
        "struct A { int id; ~A() {} }; void bad(struct A& a) { a.~A(); } int main() { return 0; }",
        "struct A { int id; ~A() {} }; int main() { ~A(); return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}
