mod common;

fn run(source: &str, stdout: &str, drops: &str) {
    let output = common::tracked_run_sizes(source);
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
fn generic_boxes_store_copy_values_and_nested_owners_and_destroy_each_once() {
    let pointer_bytes = std::mem::size_of::<usize>();
    run(
        include_str!("../examples/generic_box.cr"),
        "42\n7\n",
        &format!(
            "drop:bytes:4\ndrop:bytes:{pointer_bytes}\ndrop:bytes:{pointer_bytes}\ndrop:bytes:4\n"
        ),
    );
}

#[test]
fn replacing_box_contents_destroys_the_previous_owner_and_borrows_keep_the_box_live() {
    let definitions = include_str!("../examples/generic_box.cr")
        .split("int main()")
        .next()
        .unwrap();
    run(
        &format!(
            "{definitions} int main() {{ Box<BoxInt> value = Box<BoxInt>::create(box_int(1)); *value.get_mut() = box_int(2); print(**value.get()); Box<BoxInt> other = move(value); return 0; }}"
        ),
        "2\n",
        &format!(
            "drop:bytes:4\ndrop:bytes:4\ndrop:bytes:{}\n",
            std::mem::size_of::<usize>()
        ),
    );
    for body in [
        "Box<int> value = Box<int>::create(1); const int^ view = value.get(); Box<int> other = move(value);",
        "Box<int> value = Box<int>::create(1); const int^ view = value.get(); *value.get_mut() = 2;",
        "Box<int> value = { .data = 0 };",
    ] {
        assert!(crust::check(&format!("{definitions} int main() {{ {body} return 0; }}")).is_err());
    }
}

#[test]
fn initializing_unwritten_array_storage_skips_old_cleanup_and_destroy_reverses_elements() {
    let pointer_bytes = std::mem::size_of::<usize>();
    run(
        r#"
        extern void* malloc(size_t bytes);
        extern void free(void* pointer);
        struct Owner { BoxInt value; ~Owner() { print(*this->value); } };
        typedef Owner Owners[2];
        int main() {
            unsafe {
                Owners* data = (Owners*)malloc(sizeof(Owners));
                init(data, { { .value = box_int(1) }, { .value = box_int(2) } });
                print(*(*data)[0].value); print(*(*data)[1].value);
                destroy(data); free(data);
            }
            return 0;
        }
    "#,
        "1\n2\n2\n1\n",
        &format!(
            "drop:bytes:4\ndrop:bytes:4\ndrop:bytes:{}\n",
            pointer_bytes * 2
        ),
    );
}

#[test]
fn storage_operations_capture_the_pointer_before_evaluating_the_value() {
    run(
        r#"
        int* destination(int* value) { print(1); return value; }
        int initial() { print(2); return 42; }
        int main() { int value = 0; unsafe { init(destination(&value), initial()); destroy(destination(&value)); } print(value); return 0; }
    "#,
        "1\n2\n1\n42\n",
        "",
    );
}

#[test]
fn storage_operations_require_mutable_complete_raw_pointers_and_unsafe_contexts() {
    for body in [
        "int* pointer = 0; init(pointer, 1);",
        "int* pointer = 0; destroy(pointer);",
        "unsafe { const int* pointer = 0; init(pointer, 1); }",
        "unsafe { const int* pointer = 0; destroy(pointer); }",
        "unsafe { void* pointer = 0; init(pointer, 1); }",
        "unsafe { void* pointer = 0; destroy(pointer); }",
        "unsafe { struct Opaque* pointer = 0; destroy(pointer); }",
        "unsafe { int value = 0; init(^value, 1); }",
        "unsafe { int* pointer = 0; init(pointer, box_int(1)); }",
        "unsafe { int* pointer = 0; init(pointer); }",
        "unsafe { int* pointer = 0; destroy(pointer, 1); }",
    ] {
        assert!(
            crust::check(&format!("int main() {{ {body} return 0; }}")).is_err(),
            "should reject {body}"
        );
    }
}

#[test]
fn move_copies_plain_values_without_consuming_them_and_preserves_owner_rules() {
    run(
        r#"
        struct Pair<T> { T value; };
        T transfer<T>(T value) { return move(value); }
        int main() {
            int number = 42; int other = move(number); print(number); print(other);
            bool flag = true; bool copy = move(flag); print(flag); print(copy);
            Pair<int> first = { .value = 7 }; Pair<int> second = transfer<Pair<int>>(first); print(first.value); print(second.value);
            BoxInt owner = transfer<BoxInt>(box_int(9)); print(*owner); return 0;
        }
    "#,
        "42\n42\n1\n1\n7\n7\n9\n",
        "drop:bytes:4\n",
    );
}

#[test]
fn raw_storage_moves_transfer_owners_and_evaluate_pointer_and_index_once() {
    run(
        r#"
        extern void* malloc(size_t bytes);
        extern void free(void* pointer);
        BoxInt* source(BoxInt* data) { print(1); return data; }
        int index(BoxInt marker) { print(*marker); return 0; }
        int main() {
            unsafe {
                BoxInt* data = (BoxInt*)malloc(sizeof(BoxInt)); init(data, box_int(42));
                BoxInt marker = box_int(7);
                BoxInt value = move(source(data)[index(move(marker))]);
                free(data); print(*value);
                int number = 9; const int* pointer = &number; print(move(*pointer)); print(number);
            }
            return 0;
        }
    "#,
        "1\n7\n42\n9\n9\n",
        &format!(
            "drop:bytes:4\ndrop:bytes:{}\ndrop:bytes:4\n",
            std::mem::size_of::<usize>()
        ),
    );
    run(
        r#"
        int index(BoxInt value) { print(*value); return 0; }
        int main() { unsafe { int number = 42; int* pointer = &number; BoxInt marker = box_int(7); print(move(pointer[index(move(marker))])); } return 0; }
    "#,
        "7\n42\n",
        "drop:bytes:4\n",
    );
}

#[test]
fn raw_moves_reject_const_owners_incomplete_values_and_safe_partial_moves() {
    for body in [
        "BoxInt* pointer = 0; BoxInt value = move(*pointer);",
        "const BoxInt* pointer = 0; unsafe { BoxInt value = move(*pointer); }",
        "struct Opaque* pointer = 0; unsafe { move(*pointer); }",
        "BoxInt owner = box_int(1); BoxInt^ view = ^owner; unsafe { BoxInt value = move(*view); }",
        "BoxInt items[1] = { box_int(1) }; unsafe { BoxInt value = move(items[0]); }",
    ] {
        assert!(
            crust::check(&format!("int main() {{ {body} return 0; }}")).is_err(),
            "should reject {body}"
        );
    }
}

#[test]
fn consuming_a_generic_box_returns_its_contents_and_releases_only_its_storage() {
    let definitions = include_str!("../examples/generic_box.cr")
        .split("int main()")
        .next()
        .unwrap();
    run(
        &format!(
            "{definitions} int main() {{ Box<BoxInt> boxed = Box<BoxInt>::create(box_int(42)); BoxInt value = move(boxed).take(); print(*value); Box<int> number = Box<int>::create(7); print(move(number).take()); return 0; }}"
        ),
        "42\n7\n",
        &format!(
            "drop:bytes:{}\ndrop:bytes:4\ndrop:bytes:4\n",
            std::mem::size_of::<usize>()
        ),
    );
}
