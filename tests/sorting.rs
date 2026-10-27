mod common;

fn library() -> String {
    let vector = include_str!("../examples/container_project/src/vector.cr").replace(
        "import \"memory.cr\";",
        include_str!("../examples/container_project/src/memory.cr"),
    );
    return include_str!("../examples/container_project/src/sort.cr")
        .replace("import \"vector.cr\";", &vector);
}

fn run(source: &str, stdout: &str, stderr: &str) {
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
        stderr
    );
}

#[test]
fn sorting_example_orders_heap_owners_and_preserves_cleanup_through_growth() {
    let width = std::mem::size_of::<usize>();
    let source = include_str!("../examples/sorting.cr")
        .replace("import \"container_project/src/sort.cr\";", &library());
    run(
        &source,
        "1\n1\n2\n3\n",
        &format!(
            "drop:bytes:{}\ndrop:bytes:4\ndrop:bytes:4\ndrop:bytes:4\ndrop:bytes:4\ndrop:bytes:{}\n",
            width * 2,
            width * 4
        ),
    );
}

#[test]
fn stable_sorting_preserves_equal_key_order_and_handles_inconsistent_comparisons() {
    let source = format!(
        r#"
        {}
        struct Resource {{
            BoxInt key;
            BoxInt id;
            static Self create(int key, int id) {{ return (Self){{ .key = box_int(key), .id = box_int(id) }}; }}
            int compare(const Self^ other) const {{
                int first = *this->key; int second = *other->key;
                return (first > second) - (first < second);
            }}
            ~Resource() {{ print(*this->id); }}
        }};
        int main() {{
            Vector<Resource> values = Vector<Resource>::create(); values.reserve(4);
            values.push(Resource::create(3, 0)); values.push(Resource::create(1, 1));
            values.push(Resource::create(2, 2)); values.push(Resource::create(1, 3));
            sort<Resource>(^values);
            for (size_t i = 0; i < values.count(); i += 1) {{ print(*values.at(i)->id); }}
            return 0;
        }}
    "#,
        library()
    );
    let drops = format!(
        "{}drop:bytes:{}\n",
        "drop:bytes:4\n".repeat(8),
        std::mem::size_of::<usize>() * 8
    );
    run(&source, "1\n3\n2\n0\n0\n2\n3\n1\n", &drops);
    let inconsistent = source.replace("return (first > second) - (first < second);", "return 1;");
    let output = common::tracked_run_sizes(&inconsistent);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ids: Vec<usize> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.parse().unwrap())
        .collect();
    assert_eq!(ids.len(), 8);
    for phase in ids.chunks_exact(4) {
        let mut ids = phase.to_vec();
        ids.sort_unstable();
        assert_eq!(ids, [0, 1, 2, 3]);
    }
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        drops
    );
}

#[test]
fn sorting_empty_and_single_element_vectors_uses_checked_interface_methods() {
    let source = format!(
        r#"
        {}
        struct Item {{ int value; int compare(const Self^ other) const {{ return this->value - other->value; }} }};
        int main() {{
            Vector<Item> values = Vector<Item>::create(); sort<Item>(^values); print(values.count());
            values.push((Item){{ .value = 42 }}); sort<Item>(^values); print(values.at(0)->value);
            return 0;
        }}
    "#,
        library()
    );
    run(&source, "0\n42\n", "drop:bytes:8\n");
    let borrowed = source.replace(
        "sort<Item>(^values); print(values.at(0)->value);",
        "const Item^ view = values.at(0); sort<Item>(^values); print(view->value);",
    );
    assert!(
        crust::check(&borrowed)
            .unwrap_err()
            .message
            .contains("borrow")
    );
    for compare in [
        "",
        "int compare(const Self^ other) { return 0; }",
        "unsafe int compare(const Self^ other) const { return 0; }",
    ] {
        let source = format!(
            "{} struct Item {{ int value; {compare} }}; int main() {{ Vector<Item> values = Vector<Item>::create(); sort<Item>(^values); return 0; }}",
            library()
        );
        let error = crust::check(&source).unwrap_err();
        assert!(
            error.message.contains("interface") || error.message.contains("compare"),
            "{}",
            error.message
        );
    }
}
