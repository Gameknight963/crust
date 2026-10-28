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
fn references_alias_values_for_assignment_arithmetic_and_value_arguments() {
    run(
        r#"
        typedef int& Reference;
        int copy(int value) { return value; }
        void increment(int& value) { value++; }
        int read(const int& value) { return value; }
        int main() {
            int number = 1;
            {
                Reference reference = &number;
                reference = 10;
                reference += 2;
                print(reference++);
                print(++reference);
                increment(reference);
                increment(&reference);
                print(copy(reference));
                print(read(reference));
                print(read(&reference));
                print(sizeof(reference) == sizeof(int));
                print(sizeof(Reference) == sizeof(void*));
                { const int& view = &reference; print(view + view); }
                reference = 20;
            }
            print(number);
            return 0;
        }
    "#,
        "12\n14\n16\n16\n16\n1\n1\n32\n20\n",
        "",
    );
}

#[test]
fn references_to_pointer_slots_keep_raw_dereferencing_explicit() {
    run(
        r#"
        int* forward(int* pointer) { return pointer; }
        int main() {
            int first = 1;
            int second = 2;
            int* pointer = forward(&first);
            {
                int*& slot = &pointer;
                slot = &second;
                int** address = &slot;
                unsafe {
                    *slot += 3;
                    **address += 1;
                }
                { int* const& view = &slot; print(view != 0); }
            }
            unsafe { print(*pointer); }
            print(first);
            return 0;
        }
    "#,
        "1\n6\n1\n",
        "",
    );
}

#[test]
fn returned_references_are_places_for_stores_fields_methods_and_indexing() {
    run(
        r#"
        struct Counter {
            int value;
            void increment() { this.value += 1; }
            Self& self() { return this; }
        };
        typedef int Row[2];
        int& identity(int& value) { return value; }
        Row& row(Row& values) { return values; }
        int main() {
            int number = 1;
            identity(&number) = 3;
            identity(&number) += 2;
            print(identity(&number)++);
            print(number);
            Counter counter = { .value = 4 };
            counter.self().increment();
            counter.self().value = 7;
            print(counter.value);
            Row values = { 10, 20 };
            row(&values)[1] += 3;
            print(row(&values)[1]);
            return 0;
        }
    "#,
        "5\n6\n7\n23\n",
        "",
    );
}

#[test]
fn assigning_an_owner_through_a_reference_drops_the_previous_value_once() {
    run(
        r#"
        Box<int>& identity(Box<int>& owner) { return owner; }
        int main() {
            Box<int> owner = Box<int>::create(1);
            {
                Box<int>& reference = &owner;
                reference = Box<int>::create(2);
                identity(reference) = Box<int>::create(3);
                print(reference.get());
            }
            Box<int> next = move(owner);
            print(next.get());
            return 0;
        }
    "#,
        "3\n3\n",
        "drop:1\ndrop:2\ndrop:3\n",
    );
}

#[test]
fn raw_addresses_can_be_created_and_forwarded_without_accessing_raw_memory() {
    crust::check(
        r#"
        int* forward(int* pointer) { return pointer; }
        int main() {
            Box<int> owner = Box<int>::create(1);
            Box<int>* address = &owner;
            Box<int> next = move(owner);
            int value = 1;
            int* pointer = forward(&value);
            const int* view = &value;
            print(pointer != 0); print(view != 0); print(address != 0);
            return 0;
        }
    "#,
    )
    .unwrap();
}

#[test]
fn shared_references_and_raw_pointers_keep_their_access_restrictions() {
    for body in [
        "int n = 1; const int& r = &n; r = 2;",
        "int n = 1; const int& r = &n; r++;",
        "int n = 1; const int& r = &n; r += 2;",
        "int n = 1; const int& r = &n; int& other = &r;",
        "int n = 1; int& r = &n; print(*r);",
        "int n = 1; int* p = &n; print(*p);",
        "int n = 1; int* p = &n; int*& r = &p; print(*r);",
        "int n = 1; const int* p = &n; unsafe { *p = 2; }",
        "int n = 1; int* p = &n; print(p);",
        "int n = 1; const int& r = &n; int* p = &r;",
        "int n = 1; int& r = &n; print(n);",
        "int n = 1; int& r = &n; int& child = &r; print(r);",
        "int n = 1; const int& r = &n; n = 2;",
        "Box<int> owner = Box<int>::create(1); Box<int>& r = &owner; Box<int> copy = r;",
        "Box<int> owner = Box<int>::create(1); Box<int>& r = &owner; Box<int> next = move(r);",
        "Box<int> owner = Box<int>::create(1); Box<int>& r = &owner; Box<int> next = move(owner);",
    ] {
        let source = format!("int main() {{ {body} return 0; }}");
        assert!(crust::check(&source).is_err(), "should reject {body}");
    }
}

#[test]
fn reference_member_access_uses_dot_and_raw_member_access_uses_arrow() {
    run(
        r#"
        struct Item { int value; int read() const { return this.value; } };
        int main() {
            Item item = { .value = 1 };
            { Item& reference = &item; reference.value = 2; print(reference.read()); }
            Item* pointer = &item;
            unsafe { pointer->value = 3; print(pointer->read()); }
            return 0;
        }
    "#,
        "2\n3\n",
        "",
    );
    for body in [
        "Item& reference = &item; print(reference.get()->value);",
        "Item* pointer = &item; print(pointer.value);",
        "Item* pointer = &item; print(pointer->value);",
    ] {
        let source = format!(
            "struct Item {{ int value; }}; int main() {{ Item item = {{ .value = 1 }}; {body} return 0; }}"
        );
        assert!(crust::check(&source).is_err(), "should reject {body}");
    }
}

#[test]
fn pointer_destinations_propagate_through_conditional_addresses() {
    run(
        r#"
        const int* choose(bool first, const int& a, const int& b) {
            return first ? &a : &b;
        }
        int main() {
            int a = 1;
            int b = 2;
            int* pointer = true ? &a : &b;
            const int* view = choose(false, &a, &b);
            int* empty = false ? &a : 0;
            unsafe { *pointer = 3; print(*view); }
            print(a); print(empty == 0);
            return 0;
        }
    "#,
        "2\n3\n1\n",
        "",
    );
}

#[test]
fn shared_destinations_can_reborrow_mutable_returned_references() {
    run(
        r#"
        int& identity(int& value) { return value; }
        const int& view(int& value) { return identity(value); }
        int read(const int& value) { return value; }
        int main() {
            int number = 7;
            { const int& shared = identity(&number); print(shared); }
            print(read(identity(&number)));
            print(view(&number));
            number = 8; print(number);
            return 0;
        }
    "#,
        "7\n7\n7\n8\n",
        "",
    );
    assert!(crust::check("int& identity(int& value) { return value; } int main() { int n = 1; const int& shared = identity(&n); n = 2; return 0; }").is_err());
}

#[test]
fn printing_a_volatile_array_reference_keeps_bounded_direct_reads() {
    let source = r#"
        typedef char Text[4];
        int main() {
            Text message = "hi";
            unsafe {
                volatile Text* pointer = (volatile Text*)&message;
                const volatile Text& reference = &*pointer;
                print(reference);
            }
            return 0;
        }
    "#;
    let generated = crust::compile(source).unwrap();
    assert!(generated.contains("crust_rt_print_volatile_bytes(("));
    run(source, "hi\n", "");
}

#[test]
fn references_to_callbacks_and_copyable_structs_use_value_operations() {
    run(
        r#"
        typedef int (*Callback)(int);
        int answer(int value) { return value + 1; }
        struct Item { int value; };
        int copy(Item item) { return item.value; }
        int main() {
            Callback callback = answer;
            { Callback& reference = &callback; unsafe { print(reference(41)); } }
            Item item = { .value = 7 };
            { const Item& reference = &item; Item duplicate = reference; print(copy(reference)); print(duplicate.value); }
            return 0;
        }
    "#,
        "42\n7\n7\n",
        "",
    );
}

#[test]
fn address_operands_are_checked_and_evaluated_once() {
    run(
        r#"
        int& touch(int& value) { print(7); return value; }
        int index(Box<int> owner) { return owner.get(); }
        int main() {
            int number = 1;
            (void)&touch(&number);
            int values[1] = { 2 };
            Box<int> owner = Box<int>::create(0);
            (void)&values[index(move(owner))];
            print(number);
            return 0;
        }
    "#,
        "7\n1\n",
        "drop:0\n",
    );
}
