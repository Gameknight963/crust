mod common;

#[test]
fn forward_tags_typedefs_and_pointer_values_need_no_layout() {
    let source = r#"
        struct Handle;
        struct Handle;
        typedef struct Other Other;
        const Handle^ relay(const Handle^ value) { return value; }
        int main() {
            Handle* handle = 0;
            Other* other = 0;
            struct Implicit* implicit = 0;
            print(!handle); print(!other); print(!implicit);
            print(sizeof(Handle*) == sizeof(void*));
            print(sizeof(const Handle^) == sizeof(void*));
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
        "1\n1\n1\n1\n1\n"
    );
}

#[test]
fn forward_declarations_can_be_completed_later_in_the_unit() {
    let source = "struct Item; Item create() { return (Item){ .value = 42 }; } struct Item { int value; }; int main() { Item item = create(); print(item.value); return 0; }";
    let output = common::tracked_run(source);
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");
}

#[test]
fn opaque_types_reject_values_layout_queries_fields_and_pointer_arithmetic() {
    for source in [
        "struct Handle; int main() { Handle handle = {}; return 0; }",
        "struct Handle; Handle create() { return (Handle){}; } int main() { return 0; }",
        "struct Handle; extern Handle create(); int main() { return 0; }",
        "struct Handle; extern void take(Handle); int main() { return 0; }",
        "struct Handle; struct Owner { Handle field; }; int main() { return 0; }",
        "struct Handle; int main() { Handle values[2] = {}; return 0; }",
        "struct Handle; int main() { print(sizeof(Handle)); return 0; }",
        "struct Handle; int main() { print(alignof(Handle)); return 0; }",
        "struct Handle; int main() { Handle* p = 0; print(sizeof(*p)); return 0; }",
        "struct Handle; int main() { unsafe { Handle* p = 0; (void)*p; } return 0; }",
        "struct Handle; int main() { unsafe { Handle* p = 0; print(p->value); } return 0; }",
        "struct Handle; int main() { unsafe { Handle* p = 0; Handle* next = p + 1; } return 0; }",
        "struct Handle; int main() { unsafe { Handle* p = 0; print(p - p); } return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}
