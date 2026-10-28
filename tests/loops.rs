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
fn do_while_runs_once_and_continue_evaluates_the_condition() {
    run(
        r#"
        int condition(int& calls, int count) { calls += 1; return count < 3; }
        int main() {
            int once = 0;
            do { once += 1; } while (false);
            print(once);
            int count = 0;
            int calls = 0;
            do {
                int calls = 99;
                count += 1;
                if (count < 3) { continue; }
            } while (condition(&calls, count));
            print(count);
            print(calls);
            int total = 0;
            for (int i = 0; i < 2; i += 1) {
                int j = 0;
                do {
                    j += 1;
                    if (j == 1) { continue; }
                    do { total += 1; break; } while (true);
                    break;
                } while (j < 3);
            }
            print(total);
            return 0;
        }
        "#,
        "1\n3\n3\n2\n",
        "",
    );
}

#[test]
fn single_statement_bodies_bind_else_to_the_nearest_if_and_run_loop_steps() {
    run(
        r#"
        int select(int value) {
            if (value == 0) return 10;
            else if (value == 1) return 20;
            else return 30;
        }
        int main() {
            if (true)
                if (false) print(1);
                else print(2);
            if (false)
                if (true) print(3);
                else print(4);
            else print(5);
            print(select(0)); print(select(1)); print(select(2));
            int value = 0;
            while (value < 3) value++;
            print(value);
            do value--; while (value > 1);
            print(value);
            for (int index = 0; index < 3; index++)
                if (index == 1) continue;
                else print(index);
            int count = 0;
            do continue; while (++count < 3);
            print(count);
            while (true) break;
            for (;;) break;
            do ; while (false);
            for (; false;) ;
            return 0;
        }
    "#,
        "2\n5\n10\n20\n30\n3\n1\n0\n2\n3\n",
        "",
    );
}

#[test]
fn single_statement_bodies_preserve_return_cleanup_and_discarded_owner_cleanup() {
    run(
        r#"
        int select(bool condition) {
            Box<int> owner = Box<int>::create(10);
            if (condition) return owner.get();
            else return 20;
        }
        int main() {
            print(select(true)); print(select(false));
            int index = 0;
            while (index++ < 2) Box<int>::create(index);
            print(index);
            do Box<int>::create(index++); while (index < 5);
            print(index);
            for (index = 0; index < 2; index++) Box<int>::create(index);
            print(index);
            return 0;
        }
    "#,
        "10\n20\n3\n5\n2\n",
        "drop:10\ndrop:10\ndrop:1\ndrop:2\ndrop:3\ndrop:4\ndrop:0\ndrop:1\n",
    );
}

#[test]
fn single_statement_bodies_reject_declarations_without_braces_and_bound_nesting() {
    for body in [
        "if (true) int value = 1;",
        "if (true) ; else int value = 1;",
        "while (false) int value = 1;",
        "do int value = 1; while (false);",
        "for (;;) int value = 1;",
    ] {
        let error = crust::check(&format!("int main() {{ {body} return 0; }}")).unwrap_err();
        assert!(error.message.contains("requires braces"), "{body}: {error}");
    }
    let source = format!("int main() {{ {}return 0; }}", "if (true) ".repeat(2000));
    assert!(
        crust::check(&source)
            .unwrap_err()
            .message
            .contains("nesting limit")
    );
}

#[test]
fn do_while_cleans_body_owners_before_conditions_and_skips_conditions_on_break() {
    run(
        r#"
        struct R { Box<int> value; ~R() { print(this.value.get()); } };
        int condition(int count) { print(100 + count); return count < 5; }
        int main() {
            R outer = { .value = Box<int>::create(99) };
            int i = 0;
            do {
                R a = { .value = Box<int>::create(i) };
                {
                    R b = { .value = Box<int>::create(10 + i) };
                    i += 1;
                    if (i == 1) { continue; }
                    else if (i == 3) { break; }
                }
            } while (condition(i));
            print(42);
            return 0;
        }
        "#,
        "10\n0\n101\n11\n1\n102\n12\n2\n42\n99\n",
        "drop:10\ndrop:0\ndrop:11\ndrop:1\ndrop:12\ndrop:2\ndrop:99\n",
    );
}

#[test]
fn do_while_releases_body_borrows_before_the_condition_and_tracks_guaranteed_exits() {
    run(
        r#"
        int bump(int& value) { value += 1; return value < 2; }
        int result() {
            Box<int> owner = Box<int>::create(8);
            do { return owner.get(); } while (false);
        }
        int main() {
            int count = 0;
            do {
                const int& view = &const count;
                print(view);
                continue;
            } while (bump(&count));
            Box<int> owner = Box<int>::create(7);
            do { move(owner); break; } while (false);
            owner = Box<int>::create(9);
            print(owner.get());
            print(result());
            return 0;
        }
        "#,
        "0\n1\n9\n8\n",
        "drop:7\ndrop:8\ndrop:9\n",
    );
}

#[test]
fn do_while_rejects_invalid_syntax_scope_and_owner_backedges() {
    for body in [
        "do { move(a); } while (true);",
        "do { move(a); continue; } while (false);",
        "do { if (true) { move(a); continue; } } while (true);",
        "do { move(a); break; } while (false); print(a.get());",
        "do { } while (consume(move(a)));",
        "do { int local = 1; } while (local);",
        "do { } while (true)",
        "do { } (true);",
        "do print(1) while (true);",
    ] {
        let source = format!(
            "int consume(Box<int> owner) {{ return 0; }} int main() {{ Box<int> a = Box<int>::create(7); {body} return 0; }}"
        );
        assert!(crust::check(&source).is_err(), "should reject {source}");
    }
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
        struct R { Box<int> value; ~R() { print(this.value.get()); } };
        int main() {
            R outer = { .value = Box<int>::create(99) };
            int i = 0;
            for (R header = { .value = Box<int>::create(9) }; i < 3; i = i + 1) {
                R a = { .value = Box<int>::create(i) };
                {
                    R b = { .value = Box<int>::create(10 + i) };
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
        struct R { int id; ~R() { print(this.id); } };
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
            Box<int> a = Box<int>::create(7);
            while (1) { move(a); break; }
            a = Box<int>::create(8);
            print(a.get());
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
            Box<int> a = Box<int>::create(7);
            int i = 0;
            for (; i < 2; a = Box<int>::create(9)) {
                print(a.get());
                move(a);
                i = i + 1;
                continue;
            }
            print(a.get());
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
        "while (1) { move(a); break; } print(a.get());",
        "while (1) { move(a); continue; }",
        "while (1) { if (1) { move(a); continue; } }",
        "for (int i = 0; i < 2; i = i + 1) { move(a); continue; }",
        "for (; 1; move(a)) { continue; }",
        "while (1) { if (1) { move(a); break; } } print(a.get());",
    ] {
        let source = format!("int main() {{ Box<int> a = Box<int>::create(7); {body} return 0; }}");
        assert!(crust::check(&source).is_err(), "should reject {source}");
    }
}

#[test]
fn compound_assignments_work_on_locals_fields_and_borrowed_pointees() {
    run(
        r#"
        struct A { int n; void add(int n) { this.n += n; } };
        void add(int& n) { n += 1; }
        int main() {
            int n = 10;
            n += 5; n -= 2; n *= 3; n /= 3; n %= 5;
            n |= 8; n &= 7; n ^= 2; n += n;
            print(n);
            Box<int> owner = Box<int>::create(5);
            owner.get_mut() += 2;
            add(owner.get_mut());
            print(owner.get());
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
        int read(const int& n) { return n; }
        int main() {
            int n = 4;
            n += mark(1);
            n += read(&const n);
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
        "void bad(const int& n) { n += 1; } int main() { return 0; }",
        "int main() { Box<int> a = Box<int>::create(1); a += 1; return 0; }",
        "int main() { 1 += 2; return 0; }",
        "int replace(Box<int>& a) { a = Box<int>::create(2); return 1; } int main() { Box<int> a = Box<int>::create(1); a.get_mut() += replace(&a); return 0; }",
        "int take(Box<int> a) { return a.get(); } int main() { Box<int> a = Box<int>::create(1); a.get_mut() += take(move(a)); return 0; }",
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
        "int main() { for (int i = 0; i < 1; i++) ; print(i); return 0; }",
        "int f() { while (1) { break; } } int main() { return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}
