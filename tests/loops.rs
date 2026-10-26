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
fn for_and_while_support_break_continue_and_empty_clauses() {
    run(
        r#"
        int main() {
            int total = 0;
            for (int i = 0; i < 6; i = i + 1) {
                if (i == 1) { continue; }
                if (i == 4) { break; }
                total = total + i;
            }
            print(total);
            int n = 0;
            while (n < 5) {
                n = n + 1;
                if (n == 2) { continue; }
                if (n == 4) { break; }
                print(n);
            }
            for (;;) { break; }
            print(n);
            return 0;
        }
    "#,
        "5\n1\n3\n4\n",
        "",
    );
}

#[test]
fn for_header_and_body_owners_drop_in_scope_order_on_continue_and_break() {
    run(
        r#"
        struct R { BoxInt value; ~R() { print(*this->value); } };
        int main() {
            R outer = { .value = box_int(99) };
            int i = 0;
            for (R header = { .value = box_int(9) }; i < 3; i = i + 1) {
                R a = { .value = box_int(i) };
                {
                    R b = { .value = box_int(10 + i) };
                    if (i == 0) { continue; }
                    if (i == 1) { break; }
                }
            }
            print(100);
            return 0;
        }
    "#,
        "10\n0\n11\n1\n9\n100\n99\n",
        "drop:10\ndrop:0\ndrop:11\ndrop:1\ndrop:9\ndrop:99\n",
    );
}

#[test]
fn nested_loop_control_targets_the_innermost_loop() {
    run(
        r#"
        int main() {
            int total = 0;
            for (int i = 0; i < 2; i = i + 1) {
                for (int j = 0; j < 5; j = j + 1) {
                    if (j == 0) { continue; }
                    if (j == 2) { break; }
                    total = total + 1;
                }
                total = total + 10;
            }
            print(total);
            return 0;
        }
    "#,
        "22\n",
        "",
    );
}

#[test]
fn early_return_cleans_up_for_body_header_and_enclosing_owners() {
    run(
        r#"
        struct R { int id; ~R() { print(this->id); } };
        int f() {
            R outer = { .id = 0 };
            for (R header = { .id = 1 }; 1;) {
                R inner = { .id = 2 };
                return 42;
            }
            return 0;
        }
        int main() { print(f()); return 0; }
    "#,
        "2\n1\n0\n42\n",
        "",
    );
}

#[test]
fn a_break_can_consume_an_outer_owner_that_is_reinitialized_after_the_loop() {
    run(
        r#"
        int main() {
            BoxInt a = box_int(7);
            while (1) { move(a); break; }
            a = box_int(8);
            print(*a);
            return 0;
        }
    "#,
        "8\n",
        "drop:7\ndrop:8\n",
    );
}

#[test]
fn for_continue_runs_the_header_step_and_can_restore_an_outer_owner() {
    run(
        r#"
        int main() {
            BoxInt a = box_int(7);
            int i = 0;
            for (; i < 2; a = box_int(9)) {
                print(*a);
                move(a);
                i = i + 1;
                continue;
            }
            print(*a);
            return 0;
        }
    "#,
        "7\n9\n9\n",
        "drop:7\ndrop:9\ndrop:9\n",
    );
}

#[test]
fn continue_steps_resolve_header_bindings_in_the_presence_of_shadowing() {
    run(
        r#"
        int main() {
            for (int i = 0; i < 2; i = i + 1) {
                int i = 99;
                print(i);
                continue;
            }
            return 0;
        }
    "#,
        "99\n99\n",
        "",
    );
}

#[test]
fn unreachable_for_steps_are_checked_and_generated_but_do_not_execute() {
    run(
        r#"
        struct A { int n; static int next() { print(999); return 1; } };
        int main() {
            for (int i = 0; i < 3; i = A::next()) { break; }
            print(42);
            return 0;
        }
    "#,
        "42\n",
        "",
    );
    assert!(
        crust::check("int main() { for (int i = 0; i < 1; i = missing()) { break; } return 0; }")
            .is_err()
    );
}

#[test]
fn rejects_conflicting_owner_states_at_breaks_and_backedges() {
    for body in [
        "while (1) { move(a); break; } print(*a);",
        "while (1) { move(a); continue; }",
        "while (1) { if (1) { move(a); continue; } }",
        "for (int i = 0; i < 2; i = i + 1) { move(a); continue; }",
        "for (; 1; move(a)) { continue; }",
        "while (1) { if (1) { move(a); break; } } print(*a);",
    ] {
        let source = format!("int main() {{ BoxInt a = box_int(7); {body} return 0; }}");
        assert!(crust::check(&source).is_err(), "should reject {source}");
    }
}

#[test]
fn compound_assignments_work_on_locals_fields_and_borrowed_pointees() {
    run(
        r#"
        struct A { int n; void add(int n) { this->n += n; } };
        void add(int^ n) { *n += 1; }
        int main() {
            int n = 10;
            n += 5; n -= 2; n *= 3; n /= 3; n %= 5;
            n |= 8; n &= 7; n ^= 2; n += n;
            print(n);
            BoxInt owner = box_int(5);
            *owner += 2;
            add(^*owner);
            print(*owner);
            A a = { .n = 10 };
            a.add(4);
            print(a.n);
            int total = 0;
            for (int i = 0; i < 3; i += 1) { total += i; }
            print(total);
            return 0;
        }
    "#,
        "2\n8\n14\n3\n",
        "drop:8\n",
    );
}

#[test]
fn compound_rhs_evaluates_once_and_permits_shared_reads_of_the_destination() {
    run(
        r#"
        int mark(int n) { print(n); return n; }
        int read(const int^ n) { return *n; }
        int main() {
            int n = 4;
            n += mark(1);
            n += read(^const n);
            print(n);
            return 0;
        }
    "#,
        "1\n10\n",
        "",
    );
}

#[test]
fn compound_assignments_keep_runtime_arithmetic_checks() {
    for (source, message) in [
        (
            "int main() { int n = 2147483647; n += 1; return 0; }",
            "integer overflow",
        ),
        (
            "int main() { int n = 1; n /= 0; return 0; }",
            "division by zero",
        ),
        (
            "int main() { int n = 1; n %= 0; return 0; }",
            "remainder by zero",
        ),
    ] {
        let output = common::tracked_run(source);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains(message));
    }
}

#[test]
fn compound_rhs_cannot_mutate_move_or_replace_the_captured_destination() {
    for source in [
        "void bad(const int^ n) { *n += 1; } int main() { return 0; }",
        "int main() { BoxInt a = box_int(1); a += 1; return 0; }",
        "int main() { 1 += 2; return 0; }",
        "int replace(BoxInt^ a) { *a = box_int(2); return 1; } int main() { BoxInt a = box_int(1); *a += replace(^a); return 0; }",
        "int take(BoxInt a) { return *a; } int main() { BoxInt a = box_int(1); *a += take(move(a)); return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}

#[test]
fn rejects_loop_control_outside_loops_and_for_scope_leaks() {
    for source in [
        "int main() { break; return 0; }",
        "int main() { continue; return 0; }",
        "int main() { for (int i = 0; i < 1; i = i + 1) {} return i; }",
        "int main() { for (break; 1;) {} return 0; }",
        "int main() { for (return 0; 1;) {} return 0; }",
        "int main() { for (;;) break; return 0; }",
        "int f() { while (1) { break; } } int main() { return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}
