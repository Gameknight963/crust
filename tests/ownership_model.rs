mod common;

use std::fmt::Write;

fn random(state: &mut u64) -> usize {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    return *state as usize;
}

fn drop_all(values: &mut Vec<i32>, expected: &mut String) {
    for value in values.drain(..).rev() {
        writeln!(expected, "{value}").unwrap();
    }
}

#[test]
fn mixed_owner_operations_match_a_model_and_free_every_allocation() {
    let mut declarations = [
        include_str!("../lib/memory.cr"),
        include_str!("../lib/result.cr"),
        include_str!("../lib/allocation.cr"),
        include_str!("../lib/box.cr"),
        include_str!("../lib/vector.cr"),
    ]
    .join("\n");
    for dependency in ["memory", "result", "allocation"] {
        declarations = declarations.replace(&format!("import \"{dependency}.cr\";"), "");
    }
    declarations.push_str(
        r#"
        extern void crust_test_fail_allocations_after(size_t successes);
        struct Item {
            Box<int> value;
            static Self create(int value) { return (Self){.value=Box<int>::create(value)}; }
            ~Item() { print(this.value.get()); }
        };
        void consume(Vector<Item> items) {}
        void snapshot(const Vector<Item>& items) {
            print(items.count());
            for (size_t i=0; i<items.count(); i++) print(items.at(i).value.get());
        }
    "#,
    );
    for seed in [1, 42, 6197, 0x123456789abcdef] {
        let mut state = seed;
        let mut values = Vec::new();
        let mut expected = String::new();
        let mut source =
            format!("{declarations} int main() {{ Vector<Item> items=Vector<Item>::create();\n");
        for step in 0..256 {
            let value = step + 1000;
            match random(&mut state) % 11 {
                0 => {
                    writeln!(source, "items.push(Item::create({value}));").unwrap();
                    values.push(value);
                }
                1 => {
                    let index = random(&mut state) % (values.len() + 1);
                    writeln!(source, "items.insert({index}, Item::create({value}));").unwrap();
                    values.insert(index, value);
                }
                2 if !values.is_empty() => {
                    let index = random(&mut state) % values.len();
                    writeln!(source, "{{ Item removed=items.remove({index}); }}").unwrap();
                    writeln!(expected, "{}", values.remove(index)).unwrap();
                }
                3 if !values.is_empty() => {
                    source.push_str("{ Item removed=items.pop(); }\n");
                    writeln!(expected, "{}", values.pop().unwrap()).unwrap();
                }
                4 if !values.is_empty() => {
                    let first = random(&mut state) % values.len();
                    let second = random(&mut state) % values.len();
                    writeln!(source, "items.swap({first}, {second});").unwrap();
                    values.swap(first, second);
                }
                5 if !values.is_empty() => {
                    let index = random(&mut state) % values.len();
                    writeln!(source, "items.at_mut({index}).value.get_mut()={value};").unwrap();
                    values[index] = value;
                }
                6 => {
                    if random(&mut state).is_multiple_of(2) {
                        source.push_str("items.clear();\n");
                    } else {
                        source.push_str("consume(move(items)); items=Vector<Item>::create();\n");
                    }
                    drop_all(&mut values, &mut expected);
                }
                7 => {
                    writeln!(
                        source,
                        r#"{{
                        Vector<Item> empty=Vector<Item>::create();
                        Item incoming=Item::create({value});
                        unsafe {{ crust_test_fail_allocations_after(0); }}
                        Result<Unit, AllocationFailure<Item>> result=empty.try_push(move(incoming));
                        unsafe {{ crust_test_fail_allocations_after(~(size_t)0); }}
                        if (result.is_ok()) return 97;
                        AllocationFailure<Item> error=move(result).take_error();
                        Item recovered=move(error).take_value();
                    }}"#
                    )
                    .unwrap();
                    writeln!(expected, "{value}").unwrap();
                }
                9 if !values.is_empty() => {
                    let index = random(&mut state) % values.len();
                    writeln!(source, "items.at_mut({index})=Item::create({value});").unwrap();
                    writeln!(expected, "{}", values[index]).unwrap();
                    values[index] = value;
                }
                10 => {
                    let index = random(&mut state) % (values.len() + 1);
                    writeln!(source, "{{ Result<Unit, AllocationFailure<Item>> result=items.try_insert({index}, Item::create({value})); if (!result.is_ok()) return 98; }}").unwrap();
                    values.insert(index, value);
                }
                _ => {
                    let capacity = random(&mut state) % 32;
                    writeln!(source, "items.reserve({capacity});").unwrap();
                }
            }
            source.push_str("snapshot(&items);\n");
            writeln!(expected, "{}", values.len()).unwrap();
            for value in &values {
                writeln!(expected, "{value}").unwrap();
            }
        }
        source.push_str("return 0; }");
        drop_all(&mut values, &mut expected);
        let output = common::tracked_run(&source);
        assert!(
            output.status.success(),
            "seed {seed}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).replace('\r', ""),
            expected,
            "seed {seed}"
        );
    }
}
