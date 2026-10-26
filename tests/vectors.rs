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
fn live_element_borrows_prevent_push_pop_reserve_and_whole_vector_moves() {
    let definitions = include_str!("../examples/container_project/src/vector.cr").replace(
        "import \"memory.cr\";",
        include_str!("../examples/container_project/src/memory.cr"),
    );
    for action in [
        "values.push(2);",
        "values.pop();",
        "values.reserve(42);",
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
