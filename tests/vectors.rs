mod common;

fn run(body: &str, stdout: &str, drops: &str) {
    let definitions = include_str!("../examples/container_project/src/vector.cr").replace(
        "import \"memory.cr\";",
        include_str!("../examples/container_project/src/memory.cr"),
    );
    let source = format!("{definitions} {body}");
    let output = common::tracked_run_sizes(&source);
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
fn vectors_grow_preserve_values_pop_and_reuse_vacated_slots() {
    run(
        r#"
        int main() {
            Vector<int> values = Vector<int>::create();
            for (int i = 0; i < 5; i += 1) { values.push(i); }
            print(values.count()); print(*values.at(0)); print(values.pop());
            values.push(42); print(*values.at(4));
            *values.at_mut(1) = 7; print(*values.at(1));
            values.reserve(3); return 0;
        }
    "#,
        "5\n0\n4\n42\n7\n",
        "drop:bytes:8\ndrop:bytes:16\ndrop:bytes:32\n",
    );
}

#[test]
fn reallocation_moves_owners_without_destroying_them_and_drops_only_live_elements() {
    let width = std::mem::size_of::<usize>();
    run(
        r#"
        struct Resource {
            BoxInt value;
            ~Resource() { print(*this->value); }
        };
        int main() {
            Vector<Resource> values = Vector<Resource>::create();
            for (int i = 1; i < 4; i += 1) { values.push((Resource){ .value = box_int(i) }); }
            Resource last = values.pop(); print(*last.value);
            values.push((Resource){ .value = box_int(4) });
            Vector<Resource> next = move(values);
            return 0;
        }
    "#,
        "3\n4\n2\n1\n3\n",
        &format!(
            "drop:bytes:{}\ndrop:bytes:4\ndrop:bytes:4\ndrop:bytes:4\ndrop:bytes:{}\ndrop:bytes:4\n",
            width * 2,
            width * 4
        ),
    );
}

#[test]
fn live_element_borrows_prevent_mutation_and_whole_vector_moves() {
    let definitions = include_str!("../examples/container_project/src/vector.cr").replace(
        "import \"memory.cr\";",
        include_str!("../examples/container_project/src/memory.cr"),
    );
    for action in [
        "values.push(2);",
        "values.pop();",
        "values.reserve(42);",
        "values.insert(0, 2);",
        "values.remove(0);",
        "values.clear();",
        "values.swap(0, 0);",
        "Vector<int> other = move(values);",
    ] {
        let source = format!(
            "{definitions} int main() {{ Vector<int> values = Vector<int>::create(); values.push(1); const int^ view = values.at(0); {action} return 0; }}"
        );
        assert!(crust::check(&source).is_err(), "should reject {action}");
    }
}

#[test]
fn empty_vectors_and_fully_popped_vectors_clean_up_without_reading_unused_capacity() {
    let width = std::mem::size_of::<usize>();
    run(
        r#"
        int main() {
            Vector<BoxInt> empty = Vector<BoxInt>::create();
            Vector<BoxInt> values = Vector<BoxInt>::create(); values.reserve(10);
            values.push(box_int(42)); BoxInt value = values.pop(); print(*value);
            print(values.count()); return 0;
        }
    "#,
        "42\n0\n",
        &format!("drop:bytes:4\ndrop:bytes:{}\n", width * 10),
    );
}

#[test]
fn vector_instances_preserve_different_element_widths_and_signed_boundaries() {
    run(
        r#"
        int main() {
            Vector<int64_t> wide = Vector<int64_t>::create();
            wide.push(9223372036854775807); wide.push(-9223372036854775808); wide.push(0);
            Vector<char> bytes = Vector<char>::create(); bytes.push('a'); bytes.push('b'); bytes.push('c');
            print(*wide.at(0)); print(*wide.at(1)); print(*bytes.at(2)); print(wide.pop()); return 0;
        }
    "#,
        "9223372036854775807\n-9223372036854775808\n99\n0\n",
        "drop:bytes:16\ndrop:bytes:2\ndrop:bytes:4\ndrop:bytes:32\n",
    );
}

#[test]
fn insertion_and_removal_preserve_order_and_clear_reuses_the_allocation() {
    run(
        r#"
        int main() {
            Vector<int> values = Vector<int>::create();
            values.insert(0, 10); values.insert(1, 30); values.insert(1, 20); values.insert(0, 5);
            for (size_t i = 0; i < values.count(); i += 1) { print(*values.at(i)); }
            print(values.remove(1)); print(values.remove(2)); print(values.remove(0)); print(*values.at(0));
            values.clear(); print(values.count());
            values.push(42); print(*values.at(0));
            return 0;
        }
    "#,
        "5\n10\n20\n30\n10\n30\n5\n20\n0\n42\n",
        "drop:bytes:8\ndrop:bytes:16\n",
    );
}

#[test]
fn shifting_owning_elements_transfers_values_and_clears_each_live_element_once() {
    let width = std::mem::size_of::<usize>();
    run(
        r#"
        struct Resource {
            BoxInt value;
            ~Resource() { print(*this->value); }
        };
        int main() {
            Vector<Resource> values = Vector<Resource>::create(); values.reserve(4);
            values.push((Resource){ .value = box_int(1) });
            values.push((Resource){ .value = box_int(3) });
            values.insert(1, (Resource){ .value = box_int(2) });
            values.insert(0, (Resource){ .value = box_int(0) });
            Resource removed = values.remove(1); print(*removed.value);
            for (size_t i = 0; i < values.count(); i += 1) { print(*values.at(i)->value); }
            values.clear(); print(values.count());
            values.insert(0, (Resource){ .value = box_int(4) });
            Vector<Resource> next = move(values);
            return 0;
        }
    "#,
        "1\n0\n2\n3\n3\n2\n0\n0\n4\n1\n",
        &format!(
            "drop:bytes:4\ndrop:bytes:4\ndrop:bytes:4\ndrop:bytes:4\ndrop:bytes:{}\ndrop:bytes:4\n",
            width * 4
        ),
    );
}

#[test]
fn inserted_and_removed_arrays_keep_nested_owners_and_reverse_element_cleanup() {
    let width = std::mem::size_of::<usize>();
    run(
        r#"
        struct Resource { BoxInt value; ~Resource() { print(*this->value); } };
        typedef Resource Pair[2];
        int main() {
            Vector<Pair> values = Vector<Pair>::create(); values.reserve(3);
            Pair first = { (Resource){ .value = box_int(1) }, (Resource){ .value = box_int(2) } };
            Pair second = { (Resource){ .value = box_int(3) }, (Resource){ .value = box_int(4) } };
            values.push(move(second)); values.insert(0, move(first));
            values.swap(0, 1); values.swap(1, 1);
            Pair result = values.remove(0); print(*result[0].value); print(*result[1].value);
            values.clear(); values.clear();
            return 0;
        }
    "#,
        "3\n4\n2\n1\n4\n3\n",
        &format!(
            "drop:bytes:4\ndrop:bytes:4\ndrop:bytes:4\ndrop:bytes:4\ndrop:bytes:{}\n",
            width * 6
        ),
    );
}

#[test]
fn invalid_vector_edits_stop_before_allocation_or_element_access() {
    let definitions = include_str!("../examples/container_project/src/vector.cr").replace(
        "import \"memory.cr\";",
        include_str!("../examples/container_project/src/memory.cr"),
    );
    for (action, allocations) in [
        ("values.insert(1, 10);", 0),
        ("values.insert(~(size_t)0, 10);", 0),
        ("values.remove(0);", 0),
        ("values.swap(0, 0);", 0),
        ("values.push(1); values.swap(0, 1);", 1),
        ("values.push(1); values.swap(1, 0);", 1),
        ("values.push(1); values.swap(1, 1);", 1),
        ("values.push(1); values.insert(2, 10);", 1),
        ("values.push(1); values.remove(1);", 1),
        ("values.push(1); values.clear(); values.remove(0);", 1),
    ] {
        let source = format!(
            "{definitions} int main() {{ Vector<int> values = Vector<int>::create(); {action} return 0; }}"
        );
        let mut generated = crust::compile(&source)
            .unwrap()
            .replace("abort(", "crust_test_abort(");
        generated.push_str(&format!("void crust_test_abort(void) {{ if (live_allocations != {allocations}) {{ exit(38); }} exit(37); }}"));
        let output = common::tracked_generated_c(&generated, true);
        assert_eq!(
            output.status.code(),
            Some(37),
            "{action}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
