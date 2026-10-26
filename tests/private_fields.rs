mod common;

#[test]
fn methods_factories_and_destructors_can_access_private_fields() {
    let source = r#"
        struct Item {
            private BoxInt owner;
            int id;
            static Self create(int id) { return (Self){ .owner = box_int(id), .id = id }; }
            int read() const { return *this->owner; }
            int sum(const Self^ other) const { return *this->owner + *other->owner; }
            ~Item() { print(this->read()); }
        };
        int main() {
            Item first = Item::create(1);
            Item second = Item::create(2);
            first.id = 3;
            print(first.sum(^const second));
            return 0;
        }
    "#;
    let output = common::tracked_run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "3\n2\n1\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        "drop:2\ndrop:1\n"
    );
}

#[test]
fn private_fields_cannot_be_forged_read_written_or_borrowed_from_outside() {
    let prefix = "struct Item { private int value; static Self create() { return (Self){ .value = 1 }; } }; typedef Item Alias;";
    for body in [
        "Item item = { .value = 1 };",
        "Alias item = (Alias){ .value = 1 };",
        "Item item = Item::create(); print(item.value);",
        "Item item = Item::create(); item.value = 2;",
        "Item item = Item::create(); item.value += 1;",
        "Item item = Item::create(); int^ r = ^item.value;",
        "Item item = Item::create(); print(sizeof(item.value));",
        "Item item = Item::create(); unsafe { int* p = &item.value; }",
    ] {
        let source = format!("{prefix} int main() {{ {body} return 0; }}");
        let error = crust::check(&source).expect_err(&source);
        assert!(error.message.contains("private"), "{source}: {error}");
    }
    let source = format!(
        "{prefix} struct Other {{ int n; int read(const Item^ item) const {{ return item->value; }} }}; int main() {{ return 0; }}"
    );
    assert!(
        crust::check(&source)
            .unwrap_err()
            .message
            .contains("private")
    );
}

#[test]
fn c_owner_private_pointer_preserves_safe_construction_and_getter_invariants() {
    for definition in [
        "void forge() { HeapInt owner = { .pointer = 0 }; }",
        "void corrupt(HeapInt^ owner) { owner->pointer = 0; }",
        "void expose(const HeapInt^ owner) { print(sizeof(owner->pointer)); }",
    ] {
        let source = format!("{}\n{definition}", include_str!("../examples/c_owner.cr"));
        assert!(
            crust::check(&source)
                .unwrap_err()
                .message
                .contains("private")
        );
    }
}
