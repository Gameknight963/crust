mod common;

fn run(source: &str, expected: &str) {
    let generated = crust::compile(source).unwrap();
    let output = common::tracked_generated_c(&generated, false);
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
fn conditional_example_selects_an_owner_and_text() {
    common::assert_example(
        "examples/conditional.cr",
        "42\nlarge\n",
        "drop:7\ndrop:42\n",
        false,
    );
}

#[test]
fn conditional_precedence_associativity_and_selected_side_effects_follow_c() {
    run(
        r#"
        int bump(int& counter, int result) { counter += 1; return result; }
        void show(int value) { print(value); }
        int main() {
            int counter = 0;
            print(1 + 2 > 2 ? 42 : 0);
            print(0 || 1 ? 3 : 4);
            print(0 ? 1 : 1 ? 2 : 3);
            print(1 ? 0 ? 4 : 5 : 6);
            print((0 ? 2 : 3) + 4);
            print(bump(&counter, 1) ? bump(&counter, 8) : bump(&counter, 9));
            print(counter);
            print(0 ? 1 / 0 : 10);
            print(1 ? 11 : 1 / 0);
            true ? show(12) : show(13);
            return 0;
        }
    "#,
        "42\n3\n2\n5\n7\n8\n2\n10\n11\n12\n",
    );
}

#[test]
fn conditional_owners_merge_moves_and_drop_only_the_selected_value() {
    run(
        r#"
        T choose<T>(bool condition, T first, T second) {
            return condition ? move(first) : move(second);
        }
        int main() {
            for (int index = 0; index < 2; index += 1) {
                BoxInt first = box_int(10);
                BoxInt second = box_int(20);
                BoxInt selected = index == 0 ? move(first) : move(second);
                print(*selected);
                selected = index == 0 ? box_int(30) : box_int(40);
                print(*selected);
                BoxInt result = choose<BoxInt>(index == 0, box_int(50), box_int(60));
                print(*result);
                index == 0 ? box_int(70) : box_int(80);
                BoxInt same = box_int(90);
                BoxInt moved = index == 0 ? move(same) : move(same);
                print(*moved);
            }
            return 0;
        }
    "#,
        "10\n30\n50\n90\n20\n40\n60\n90\n",
    );
}

#[test]
fn conditional_values_preserve_numeric_enum_pointer_callback_and_aggregate_types() {
    run(
        r#"
        struct Pair { int value; };
        enum Color { Red, Blue };
        typedef int Callback(int);
        int increment(int value) { return value + 1; }
        int decrement(int value) { return value - 1; }
        int main() {
            print(true ? (uint8_t)255 : (int64_t)-1);
            Color color = false ? Red : Blue;
            print(color == Blue);
            Pair pair = true ? (Pair){ .value = 42 } : (Pair){ .value = 1 };
            print(pair.value);
            int array_a[2] = { 1, 2 };
            int array_b[2] = { 3, 4 };
            int selected[2] = false ? array_a : array_b;
            print(selected[1]);
            int number = 42;
            unsafe {
                int* plain = &number;
                const volatile int* qualified = plain;
                const volatile int* merged = true ? plain : qualified;
                print(*merged);
                const int* nullable = false ? 0 : plain;
                print(*nullable);
                void* erased = plain;
                const volatile void* merged_void = false ? qualified : erased;
                print((bool)merged_void);
                Callback* callback = true ? increment : decrement;
                print(callback(41));
            }
            print(sizeof(true ? (uint8_t)1 : (int64_t)2));
            BoxInt owned = box_int(9);
            print(sizeof(true ? move(owned) : move(owned)));
            print(*owned);
            return 0;
        }
    "#,
        &format!(
            "255\n1\n42\n4\n42\n42\n1\n42\n8\n{}\n9\n",
            std::mem::size_of::<usize>()
        ),
    );
}

#[test]
fn conditional_constants_short_circuit_enum_values_and_array_lengths() {
    run(
        r#"
        enum Values { Count = 1 ? 3 : 1 / 0, Value = 0 ? 1 / 0 : Count * 14 };
        typedef int Items[true ? Count : 1];
        int main() {
            Items items = { 1, 2, 3 };
            print(items[2]);
            print((int)Value);
            return 0;
        }
    "#,
        "3\n42\n",
    );
    for source in [
        "enum Values { A = 1 ? 3 : missing }; int main() { return 0; }",
        "enum Values { A = 1 ? 1 / 0 : 3 }; int main() { return 0; }",
    ] {
        assert!(crust::check(source).is_err());
    }
}

#[test]
fn conditional_expressions_reject_invalid_types_partial_moves_and_borrow_results() {
    for source in [
        "int main() { int value = 1 ? 2; return value; }",
        "int main() { BoxInt value = box_int(1); print(value ? 1 : 2); return 0; }",
        "int main() { BoxInt first = box_int(1); BoxInt second = box_int(2); BoxInt selected = true ? move(first) : move(second); print(*first); return 0; }",
        "int main() { int first = 1; int second = 2; const int& selected = true ? &const first : &const second; return 0; }",
        "int main() { print(true ? box_int(1) : 2); return 0; }",
        "extern void show(); int main() { unsafe { true ? show() : 1; } return 0; }",
        "int main() { int number = 1; unsafe { int* pointer = true ? &number : 3; } return 0; }",
        "int use(const BoxInt& first, BoxInt second) { return *first + *second; } int main() { BoxInt owner = box_int(1); return use(&const owner, true ? move(owner) : box_int(2)); }",
    ] {
        assert!(crust::check(source).is_err(), "{source}");
    }
    let deep = format!("int main() {{ return {}0; }}", "1 ? 1 : ".repeat(2000));
    assert!(
        crust::check(&deep)
            .unwrap_err()
            .message
            .contains("nesting limit")
    );
}
