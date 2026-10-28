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
fn c_allocation_can_be_wrapped_in_a_safe_move_only_owner() {
    run(include_str!("../examples/c_owner.cr"), "42\n", "drop:42\n");
}

#[test]
fn void_pointer_locals_support_null_nested_indirection_and_for_initializers() {
    run(
        r#"
        int main() {
            void* empty = 0;
            print(empty == 0);
            unsafe {
                int value = 42;
                void* pointer = &value;
                void** slot = &pointer;
                print(*(int*)*slot);
                for (void* current = pointer; current != 0; current = 0) {
                    print(*(int*)current);
                }
            }
            return 0;
        }
    "#,
        "1\n42\n42\n",
        "",
    );
    let error = crust::check("int main() { void value = 0; return 0; }").unwrap_err();
    assert!(
        error
            .message
            .contains("a local variable cannot have type void")
    );
}

#[test]
fn named_struct_and_typedef_pointer_slots_support_mutable_and_shared_borrows() {
    run(
        r#"
        struct Item { int value; };
        typedef Item Named;
        void set_pointer<T>(T*& target, T* next) {
            T*& slot = &target;
            slot = next;
        }
        int main() {
            Item first = { .value = 7 };
            Item second = { .value = 42 };
            unsafe {
                Item* pointer = &first;
                { Item*& slot = &pointer; set_pointer<Item>(&slot, &second); print((slot)->value); }
                { Named* const& slot = &const pointer; print((slot)->value); }
                pointer = &first; print(pointer->value);
            }
            return 0;
        }
    "#,
        "42\n42\n7\n",
        "",
    );
}

#[test]
fn pointer_slot_borrows_preserve_shared_permissions_and_exclusive_root_loans() {
    for (declaration, action) in [
        ("Handle* const& slot = &const pointer;", "slot = 0;"),
        ("Handle*& slot = &pointer;", "pointer = 0;"),
    ] {
        let source = format!(
            "struct Handle; int main() {{ Handle* pointer = 0; {declaration} {action} return 0; }}"
        );
        let error = crust::check(&source).unwrap_err();
        assert!(error.message.contains("borrow"), "{}", error.message);
    }
}

#[test]
fn raw_pointers_support_nested_indirection_and_safe_borrow_bindings() {
    run(
        r#"
        typedef int* Pointer;
        int main() {
            int value = 1;
            unsafe {
                Pointer p = &value;
                int** pp = &p;
                **pp = 2;
                {
                    Pointer& slot = &p;
                    *slot += 1;
                }
                const int* view = p;
                print(*view); print(*p);
                if (p != view || view != p) { return 1; }
                const void* opaque = view;
                if (p != opaque || opaque != p) { return 2; }
                uintptr_t bits = (uintptr_t)p;
                int* restored = (int*)bits;
                print(restored == p);
                int* empty = 0;
                print(!empty); print(p && !empty); print(0 == empty);
            }
            print(value);
            return 0;
        }
    "#,
        "3\n3\n1\n1\n1\n1\n3\n",
        "",
    );
}

#[test]
fn calls_c_runtime_functions_with_native_char_pointers_and_unnamed_parameters() {
    run(
        r#"
        extern int puts(const char*);
        extern size_t strlen(const char*);
        int main() {
            char text[6] = "Crust";
            unsafe {
                puts(&text[0]);
                print(strlen(&text[0]));
            }
            return 0;
        }
    "#,
        "Crust\n5\n",
        "",
    );
}

#[test]
fn unsafe_blocks_scope_owners_and_preserve_cleanup_on_return_and_loop_control() {
    run(
        r#"
        int get() {
            unsafe {
                Box<int> owner = Box<int>::create(7);
                int* pointer = &owner.get_mut();
                return *pointer;
            }
        }
        int main() {
            for (int i = 0; i < 2; i += 1) {
                unsafe {
                    Box<int> owner = Box<int>::create(i);
                    if (i == 0) { continue; }
                    break;
                }
            }
            print(get());
            return 0;
        }
    "#,
        "7\n",
        "drop:0\ndrop:1\ndrop:7\n",
    );
}

#[test]
fn raw_pointer_operations_and_c_calls_require_explicit_unsafe_contexts() {
    for source in [
        "int read(int* p) { return *p; } int main() { return 0; }",
        "int main() { int* p = 0; *p = 1; return 0; }",
        "int main() { int* p = 0; int& r = &*p; return 0; }",
        "int main() { int* p = (int*)0; return 0; }",
        "int main() { int* p = 0; uintptr_t bits = (uintptr_t)p; return 0; }",
        "extern void abort(); int main() { abort(); return 0; }",
    ] {
        let error = crust::check(source).expect_err(source);
        assert!(error.message.contains("unsafe block"), "{source}: {error}");
    }
}

#[test]
fn pointer_type_checks_reject_invalid_casts_mutability_and_c_signatures() {
    for source in [
        "int main() { unsafe { int* p = (int*)Box<int>::create(1); } return 0; }",
        "int main() { unsafe { int* p = (int*)42; } return 0; }",
        "int main() { unsafe { int* p = 0; int value = (int)p; } return 0; }",
        "int main() { unsafe { void* p = 0; print(*p); } return 0; }",
        "int main() { unsafe { const int* p = 0; *p = 1; } return 0; }",
        "int main() { unsafe { const int* p = 0; int* q = p; } return 0; }",
        "int main() { unsafe { const int* p = 0; void* q = p; } return 0; }",
        "extern Box<int> make(); int main() { return 0; }",
        "extern void take(int&); int main() { return 0; }",
        "extern void take(int values[2]); int main() { return 0; }",
        "extern int main();",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}

#[test]
fn unevaluated_size_queries_allow_raw_pointer_types_without_accessing_memory() {
    run(
        r#"
        extern int never_called(int*);
        int main() {
            int* pointer = 0;
            print(sizeof(*pointer));
            print(sizeof(never_called(pointer)));
            print(sizeof(int*));
            return 0;
        }
    "#,
        &format!("4\n4\n{}\n", std::mem::size_of::<usize>()),
        "",
    );
}

#[test]
fn pointer_arithmetic_indexing_and_compound_steps_follow_c_element_sizes() {
    run(
        r#"
        int main() {
            int values[4] = { 10, 20, 30, 40 };
            unsafe {
                int* start = &values[0];
                int* middle = start + 2;
                print(*middle); print(middle[-1]);
                print(middle - start);
                print(*(1 + start));
                middle -= 1; print(*middle);
                middle += 2; print(*middle);
                *(middle - 1) = 99;
                ((int*)&values[0])[0] = 5;
                print(start[0]); print(start[2]);
                const int* view = start;
                print(view[3]); print(middle - view);
                int one = 1;
                *(start + -(-one)) = 8;
                print(start[1]);
                *(start + (start != 0)) += 1;
                print(start[1]);
                *(start + sizeof(char)) = 11;
                print(start[1]);
            }
            return 0;
        }
    "#,
        "30\n20\n2\n20\n20\n40\n5\n99\n40\n3\n8\n9\n11\n",
        "",
    );
}

#[test]
fn pointer_index_bases_and_indices_evaluate_once_in_assignment_order() {
    run(
        r#"
        int mark(int n) { print(n); return n; }
        int* base(int* p, int n) { print(n); return p; }
        int main() {
            int values[3] = { 0, 0, 0 };
            unsafe {
                int* p = &values[0];
                base(p, 1)[mark(2)] = mark(3);
                base(p, 1)[mark(2)] += mark(3);
                print(p[2]);
            }
            return 0;
        }
    "#,
        "3\n1\n2\n1\n2\n3\n6\n",
        "",
    );
}

#[test]
fn pointer_offsets_reject_values_outside_ptrdiff_range() {
    for operation in ["p[index]", "*(p + index)"] {
        let source = format!(
            "int main() {{ int n = 1; unsafe {{ int* p = &n; uint64_t index = 18446744073709551615; print({operation}); }} return 0; }}"
        );
        let output = common::tracked_run(&source);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("integer conversion out of range")
        );
    }
    for source in [
        "int main() { int* p = 0; print(p[0]); return 0; }",
        "int main() { int* p = 0; int* q = p + 1; return 0; }",
        "int main() { int* p = 0; p += 1; return 0; }",
        "int main() { int* p = 0; print(p - p); return 0; }",
        "int main() { unsafe { void* p = 0; print(p[0]); } return 0; }",
        "int main() { unsafe { void* p = 0; void* q = p + 1; } return 0; }",
        "int main() { unsafe { const int* p = 0; p[0] = 1; } return 0; }",
        "int main() { unsafe { int* p = 0; char* q = 0; print(p - q); } return 0; }",
        "int main() { unsafe { int* p = 0; p *= 2; } return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}

#[test]
fn c_backed_buffer_has_checked_access_and_automatic_cleanup() {
    run(
        include_str!("../examples/int_buffer.cr"),
        "10\n20\n30\n",
        "drop:10\n",
    );
    let definitions = include_str!("../examples/int_buffer.cr")
        .split("int main()")
        .next()
        .unwrap();
    run(
        &format!(
            "{definitions} int main() {{ IntBuffer empty = IntBuffer::create(0); print(empty.count()); IntBuffer values = IntBuffer::create(2); {{ int& first = values.at_mut(0); first = 7; }} IntBuffer moved = move(values); print(moved.at(1)); return 0; }}"
        ),
        "0\n0\n",
        "drop:7\n",
    );
    let output = common::tracked_run(&format!(
        "{definitions} int main() {{ IntBuffer values = IntBuffer::create(2); print(values.at(2)); return 0; }}"
    ));
    assert!(
        !output.status.success(),
        "out-of-bounds access must terminate before reading memory"
    );
    for body in [
        "IntBuffer values = IntBuffer::create(2); int& view = values.at_mut(0); IntBuffer next = move(values);",
        "IntBuffer values = IntBuffer::create(2); const int& view = values.at(0); values = IntBuffer::create(3);",
        "IntBuffer values = { .data = 0, .length = 3 };",
    ] {
        assert!(crust::check(&format!("{definitions} int main() {{ {body} return 0; }}")).is_err());
    }
}
