mod common;

fn run(source: &str, expected: &str) {
    let output = common::tracked_generated_c(&crust::compile(source).unwrap(), false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        expected
    );
}

#[test]
fn update_example_runs_loop_steps_and_array_updates() {
    run(
        include_str!("../examples/updates.cr"),
        "10\n20\n30\n41\n42\n",
    );
}

#[test]
fn updates_return_old_or_new_values_and_evaluate_destinations_once() {
    run(
        r#"
        struct Item { int value; };
        T next<T>(T value) { return ++value; }
        int main() {
            int value = 1;
            print(value++ + ++value);
            print(value);
            print(value--);
            print(--value);
            int values[2] = { 10, 20 };
            int index = 0;
            print(values[index++]++);
            print(index);
            print(values[0]);
            Item item = { .value = 40 };
            print(++item.value);
            print(item.value++);
            print(item.value);
            int total = 0;
            for (int i = 0; i < 3; i++) { total += i; }
            print(total);
            print(next<int>(41));
            print(next<char>('a'));
            print(sizeof(value++));
            print(value);
            return 0;
        }
    "#,
        "4\n3\n3\n1\n10\n1\n11\n41\n41\n42\n3\n42\n98\n4\n1\n",
    );
}

#[test]
fn pointer_updates_follow_postfix_and_unary_precedence() {
    run(
        r#"
        int main() {
            int values[3] = { 10, 20, 30 };
            unsafe {
                int* pointer = &values[0];
                print(*pointer++);
                print(*++pointer);
                print(*--pointer);
                print(*pointer--);
                print(*pointer);
                const int* readonly = &values[0];
                print(*readonly++);
                print(*readonly);
            }
            return 0;
        }
    "#,
        "10\n30\n20\n20\n10\n10\n20\n",
    );
}

#[test]
fn updates_through_borrows_and_volatile_views_preserve_owner_cleanup() {
    run(
        r#"
        int main() {
            BoxInt owner = box_int(40);
            print((*owner)++);
            { int& view = &*owner; print(++view); }
            print(*owner);
            int value = 10;
            unsafe {
                volatile int* view = &value;
                print((*view)++);
                print(++*view);
                print((*view)--);
                print(--*view);
            }
            print(value);
            return 0;
        }
    "#,
        "40\n42\n42\n10\n12\n12\n10\n10\n",
    );
}

#[test]
fn updates_reject_readonly_noninteger_and_incomplete_destinations_and_conflicting_loans() {
    for source in [
        "int main() { ++1; return 0; }",
        "int main() { int value = 1; value++++; return 0; }",
        "int main() { BoxInt owner = box_int(1); owner++; return 0; }",
        "int main() { bool flag = true; flag++; return 0; }",
        "enum Color { Red }; int main() { Color color = Red; ++color; return 0; }",
        "int main() { int value = 1; const int& shared = &const value; (shared)++; return 0; }",
        "int main() { int value = 1; const int& shared = &const value; value++; print(shared); return 0; }",
        "int use(int& first, int second) { return first + second; } int main() { int value = 1; return use(&value, value++); }",
        "int main() { int* pointer = 0; pointer++; return 0; }",
        "int main() { void* pointer = 0; unsafe { pointer++; } return 0; }",
        "struct Handle; int main() { Handle* pointer = 0; unsafe { ++pointer; } return 0; }",
        "typedef int Callback(int); int main() { Callback* pointer = 0; unsafe { pointer++; } return 0; }",
        "int main() { int value = value++; return 0; }",
        "enum Values { Value = 1++ }; int main() { return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "{source}");
    }
}

#[test]
fn updates_check_overflow_and_narrowing_and_wrap_unsigned_wide_values() {
    run(
        "int main() { uint64_t value = 18446744073709551615; print(value++); print(value); print(--value); return 0; }",
        "18446744073709551615\n0\n18446744073709551615\n",
    );
    for declaration in [
        "int value = 2147483647; value++;",
        "int value = -2147483648; --value;",
        "uint8_t value = 255; ++value;",
    ] {
        let source = format!("int main() {{ {declaration} return 0; }}");
        let output = common::tracked_generated_c(&crust::compile(&source).unwrap(), false);
        assert!(!output.status.success(), "{source}");
        assert!(!output.stderr.is_empty(), "{source}");
    }
}

#[test]
fn update_nesting_is_bounded_for_prefix_and_postfix_chains() {
    for expression in [
        format!("{}value", "++".repeat(2000)),
        format!("value{}", "++".repeat(2000)),
    ] {
        let source = format!("int main() {{ int value = 1; {expression}; return 0; }}");
        assert!(
            crust::check(&source)
                .unwrap_err()
                .message
                .contains("nesting limit")
        );
    }
}
