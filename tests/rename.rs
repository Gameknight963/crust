use crust::analysis::{Analysis, TextEdit};
mod common;
use common::compiler as crust;

fn apply(analysis: &Analysis, edits: &[TextEdit]) -> Vec<String> {
    let mut texts: Vec<_> = analysis
        .files
        .iter()
        .map(|file| file.text.clone())
        .collect();
    for edit in edits.iter().rev() {
        texts[edit.location.file].replace_range(
            edit.location.span.start..edit.location.span.end,
            &edit.new_text,
        );
    }
    return texts;
}

#[test]
fn exported_function_rename_preserves_native_names_and_ignores_body_assignments() {
    let source = "extern int calculate(int value) { int result = value; result = result + 1; return result; } extern int native_call(); int main() { unsafe { print(native_call()); print(calculate(1)); } return 0; }";
    let analysis = crust::analyze(source).unwrap();
    let edits = analysis
        .rename(0, source.find("calculate(int").unwrap(), "increment")
        .unwrap();
    let updated = apply(&analysis, &edits).remove(0);
    assert!(
        updated.contains("increment(int value) = \"calculate\" {"),
        "{updated}"
    );
    let c = format!(
        "{}\nint32_t native_call(void) {{ return calculate(41); }}",
        crust::compile(&updated).unwrap()
    );
    let output = common::tracked_generated_c(&c, false);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n2\n"
    );
}

#[test]
fn method_rename_follows_receivers_recursion_and_static_callback_addresses() {
    let source = r#"
        typedef int Callback(int);
        struct Counter {
            int value;
            int read(int depth) const {
                if (depth == 0) { return this.value; }
                return this.read(depth - 1);
            }
            static int double_value(int value) { return value * 2; }
        };
        struct Other { int value; int read(int depth) const { return depth; } };
        int read(int value) { return value; }
        int main() {
            Counter counter = { .value = 21 };
            const Counter& view = &const counter;
            Other other = { .value = 0 };
            print(counter.read(1));
            print(view.read(0));
            print(other.read(7));
            print(read(8));
            Callback* callback = Counter::double_value;
            unsafe { print(callback(21)); }
            print(Counter::double_value(3));
            return 0;
        }
    "#;
    let analysis = crust::analyze(source).unwrap();
    let edits = analysis
        .rename(0, source.find("read(int depth)").unwrap(), "sample")
        .unwrap();
    let renamed = apply(&analysis, &edits).remove(0);
    assert!(renamed.contains("this.sample(depth - 1)"));
    assert!(renamed.contains("counter.sample(1)"));
    assert!(renamed.contains("view.sample(0)"));
    assert!(renamed.contains("other.read(7)"));
    assert!(renamed.contains("print(read(8))"));
    let analysis = crust::analyze(&renamed).unwrap();
    let edits = analysis
        .rename(0, renamed.find("double_value(int").unwrap(), "twice")
        .unwrap();
    let renamed = apply(&analysis, &edits).remove(0);
    assert!(renamed.contains("callback = Counter::twice"));
    assert!(renamed.contains("Counter::twice(3)"));
    let output = common::tracked_generated_c(&crust::compile(&renamed).unwrap(), false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "21\n21\n7\n8\n42\n6\n"
    );
}

#[test]
fn method_rename_covers_generic_instances_and_rejects_collisions_and_unresolved_calls() {
    let source = "struct Item<T> { T value; T get() const { return this.value; } }; int main() { Item<int> first = { .value = 42 }; Item<char> second = { .value = 'a' }; print(first.get()); print(second.get()); return 0; }";
    let analysis = crust::analyze(source).unwrap();
    let edits = analysis
        .rename(0, source.find("get()").unwrap(), "read")
        .unwrap();
    let renamed = apply(&analysis, &edits).remove(0);
    assert!(renamed.contains("T read()"));
    assert!(renamed.contains("first.read()"));
    assert!(renamed.contains("second.read()"));
    crust::check(&renamed).unwrap();

    for source in [
        "struct Item { int value; int get() const { return 1; } int read() const { return 2; } }; int main() { Item item = { .value = 0 }; return item.get(); }",
        "struct Item { int value; int get() const { return 1; } }; int later<T>(const T& value) { return value.get(); } int main() { Item item = { .value = 0 }; return item.get(); }",
        "interface Readable { int get() const; }; struct Item { int value; int get() const { return 1; } }; int sample<T: Readable>(const T& value) { return value.get(); } int main() { Item item = { .value = 0 }; return sample<Item>(&const item); }",
    ] {
        let analysis = crust::analyze(source).unwrap();
        let start = source.find("struct Item").unwrap();
        let offset = start + source[start..].find("get()").unwrap();
        assert!(analysis.rename(0, offset, "read").is_err(), "{source}");
    }
    let source = "struct Item<T> { T value; ~Item() {} }; int main() { Item<int> item = { .value = 1 }; return 0; }";
    let analysis = crust::analyze(source).unwrap();
    assert!(
        analysis
            .rename_target(0, source.find("~Item").unwrap() + 1)
            .is_some_and(|(symbol, _)| symbol.kind == crust::analysis::SymbolKind::Struct)
    );
}

#[test]
fn renamed_consuming_methods_keep_owner_cleanup() {
    let source = "struct Item { Box<int> value; int take(Self this) { return this.value.get(); } }; int main() { Item item = { .value = Box<int>::create(42) }; print(move(item).take()); return 0; }";
    let analysis = crust::analyze(source).unwrap();
    let edits = analysis
        .rename(0, source.find("take(Self").unwrap(), "consume")
        .unwrap();
    let renamed = apply(&analysis, &edits).remove(0);
    let output = common::tracked_generated_c(&crust::compile(&renamed).unwrap(), false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n"
    );
}

#[test]
fn typedef_rename_follows_redeclarations_callbacks_generics_and_type_expressions() {
    let source = r#"
        typedef int Count;
        typedef int Count;
        typedef Count Callback(Count);
        struct Item { Count value; };
        Count identity(Count value) { return value; }
        T echo<T>(T value) { return value; }
        Count sum(Count first, Count second) { return first + second; }
        int main() {
            Count values[2] = { 20, 22 };
            Item item = { .value = sum(values[0], values[1]) };
            Count value = echo<Count>(item.value);
            print((Count)value);
            print(sizeof(Count));
            int Count = 7;
            print(Count);
            Callback* callback = identity;
            unsafe { print(callback(value)); }
            return 0;
        }
    "#;
    let analysis = crust::analyze(source).unwrap();
    let edits = analysis
        .rename(0, source.find("Count;").unwrap(), "Amount")
        .unwrap();
    let renamed = apply(&analysis, &edits).remove(0);
    assert!(renamed.contains("typedef int Amount;"));
    assert!(renamed.contains("typedef Amount Callback(Amount)"));
    assert!(renamed.contains("echo<Amount>"));
    assert!(renamed.contains("int Count = 7;"));
    assert!(renamed.contains("print(Count)"));
    let output = common::tracked_generated_c(&crust::compile(&renamed).unwrap(), false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n4\n7\n42\n"
    );
}

#[test]
fn typedef_rename_rejects_type_capture_and_unresolved_template_uses() {
    for source in [
        "typedef int Count; typedef char Amount; int main() { Count value = 1; return value; }",
        "typedef int Count; T later<T>(Count count, T value) { return value; } int main() { Count value = 1; return value; }",
    ] {
        let analysis = crust::analyze(source).unwrap();
        assert!(
            analysis
                .rename(0, source.find("Count;").unwrap(), "Amount")
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn typedef_rename_preserves_native_opaque_tags_and_function_symbols() {
    let source = "struct NativeHandle; typedef NativeHandle Handle; extern int native_read(const Handle*); int main() { unsafe { Handle* handle = 0; print(native_read(handle)); } return 0; }";
    let analysis = crust::analyze(source).unwrap();
    let edits = analysis
        .rename(0, source.find(" Handle;").unwrap() + 1, "Resource")
        .unwrap();
    let renamed = apply(&analysis, &edits).remove(0);
    assert!(renamed.contains("struct NativeHandle;"));
    assert!(renamed.contains("typedef NativeHandle Resource;"));
    let c = format!(
        "{}\nint native_read(const struct NativeHandle* handle) {{ return handle == NULL ? 42 : 0; }}",
        crust::compile(&renamed).unwrap()
    );
    let output = common::tracked_generated_c(&c, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n"
    );
}

#[test]
fn interface_rename_updates_composed_constraints_and_keeps_implementing_methods() {
    let source = r#"
        interface Readable { int read() const; };
        static interface Factory { Self create(int value); };
        struct Item {
            int value;
            int read() const { return this.value; }
            static Self create(int value) { return (Self){ .value = value }; }
        };
        struct Wrapper<T: Readable> { T item; };
        int sample<T: Readable>(const T& value) { return value.read(); }
        int produce<T: Factory + Readable>(int value) {
            T result = T::create(value);
            return sample<T>(&const result);
        }
        int main() {
            Wrapper<Item> wrapped = { .item = Item::create(7) };
            print(sample<Item>(&const wrapped.item));
            print(produce<Item>(42));
            return 0;
        }
    "#;
    let analysis = crust::analyze(source).unwrap();
    assert!(
        analysis
            .rename_target(0, source.find("Self create").unwrap())
            .is_none()
    );
    let edits = analysis
        .rename(0, source.find("Readable {").unwrap(), "Inspectable")
        .unwrap();
    let renamed = apply(&analysis, &edits).remove(0);
    assert!(!renamed.contains("Readable"));
    assert!(renamed.contains("int read() const"));
    assert!(renamed.contains("value.read()"));
    let analysis = crust::analyze(&renamed).unwrap();
    let edits = analysis
        .rename(0, renamed.find("Factory {").unwrap(), "Creator")
        .unwrap();
    let renamed = apply(&analysis, &edits).remove(0);
    assert!(renamed.contains("static interface Creator"));
    assert!(renamed.contains("T: Creator + Inspectable"));
    let output = common::tracked_generated_c(&crust::compile(&renamed).unwrap(), false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "7\n42\n"
    );
}

#[test]
fn interface_rename_rejects_existing_type_names_and_keeps_unused_constraint_references() {
    let source = "interface Readable { int read() const; }; interface Other { int get() const; }; int future<T: Readable>(const T& value) { return value.read(); } int main() { return 0; }";
    let analysis = crust::analyze(source).unwrap();
    let offset = source.find("Readable {").unwrap();
    assert!(analysis.rename(0, offset, "Other").is_err());
    let edits = analysis.rename(0, offset, "Inspectable").unwrap();
    let renamed = apply(&analysis, &edits).remove(0);
    assert!(renamed.contains("future<T: Inspectable>"));
    assert!(renamed.contains("value.read()"));
    crust::check(&renamed).unwrap();
}

#[test]
fn renames_only_the_selected_local_and_preserves_comments_strings_and_shadowing() {
    let source = "int main() { int value = 1; /* value */ { int value = 2; print(value); } print(\"value\"); value = value + 1; return value; }";
    let analysis = crust::analyze(source).unwrap();
    let offset = source.find("value = 1").unwrap();
    for name in ["longer_name", "v"] {
        let edits = analysis.rename(0, offset, name).unwrap();
        assert_eq!(edits.len(), 4);
        let result = apply(&analysis, &edits).remove(0);
        assert!(result.contains("/* value */ { int value = 2; print(value); } print(\"value\");"));
        assert!(result.contains(&format!("{name} = {name} + 1; return {name};")));
        crust::check(&result).unwrap();
    }
    assert!(analysis.rename(0, offset, "value").unwrap().is_empty());
}

#[test]
fn rejects_capture_of_renamed_and_preexisting_references() {
    for (source, selected, name) in [
        (
            "int main() { int original = 1; { int taken = 2; print(original); } return original; }",
            "original =",
            "taken",
        ),
        (
            "int main() { int first = 1; { int renamed = 2; print(first); } return first; }",
            "renamed =",
            "first",
        ),
    ] {
        let analysis = crust::analyze(source).unwrap();
        let error = analysis
            .rename(0, source.find(selected).unwrap(), name)
            .unwrap_err();
        assert!(error.contains("which declaration"), "{error}");
    }
}

#[test]
fn rejects_duplicate_declarations_invalid_names_and_unsupported_targets() {
    let source = "int main() { int first = 1; int second = 2; return first + second; }";
    let analysis = crust::analyze(source).unwrap();
    let offset = source.find("first =").unwrap();
    assert!(
        analysis
            .rename(0, offset, "second")
            .unwrap_err()
            .contains("already declared")
    );
    for name in [
        "",
        "int",
        "return",
        "1value",
        "two words",
        " value",
        "value ",
        "value/*comment*/",
        "🦀",
    ] {
        assert!(analysis.rename(0, offset, name).is_err(), "{name}");
    }
    assert!(
        analysis
            .rename_target(0, source.find("main").unwrap())
            .is_none()
    );
    assert!(
        analysis
            .rename(0, source.find("main").unwrap(), "entry")
            .is_err()
    );
    assert!(analysis.rename_target(0, source.len() + 1).is_none());
}

#[test]
fn renames_generic_parameters_as_value_bindings_without_touching_type_parameters() {
    let source = "T identity<T>(T T) { return T; } int main() { print(identity<int>(1)); print(identity<char>('a')); return 0; }";
    let analysis = crust::analyze(source).unwrap();
    let offset = source.find("T) {").unwrap();
    let edits = analysis.rename(0, offset, "value").unwrap();
    assert_eq!(edits.len(), 2);
    let result = apply(&analysis, &edits).remove(0);
    assert!(result.starts_with("T identity<T>(T value) { return value; }"));
    let edits = analysis
        .rename(0, source.find("T>(T").unwrap(), "Value")
        .unwrap();
    let result = apply(&analysis, &edits).remove(0);
    assert!(result.starts_with("Value identity<Value>(Value T) { return T; }"));
}

#[test]
fn field_rename_follows_generic_initializers_borrows_and_members_without_touching_other_types() {
    let source = r#"
struct Item<T> { T value; T read() const { return this.value; } };
struct Other { int value; };
int main() {
    Item<int> items[2] = { { .value = 41 }, { .value = 42 } };
    Item<char> letter = { .value = 'a' };
    Other other = { .value = 7 };
    int value = 8;
    { int& view = &items[1].value; view += 1; }
    print(items[1].read());
    print(letter.read());
    print(other.value);
    print(value);
    return 0;
}
"#;
    let analysis = crust::analyze(source).unwrap();
    let edits = analysis
        .rename(0, source.find("T value;").unwrap() + 2, "data")
        .unwrap();
    let renamed = apply(&analysis, &edits).remove(0);
    assert!(renamed.contains("T data;"));
    assert!(renamed.contains("this.data"));
    assert!(renamed.contains("&items[1].data"));
    assert!(renamed.contains("Other other = { .value = 7 }"));
    assert!(renamed.contains("print(other.value)"));
    assert!(renamed.contains("int value = 8;"));
    let output = common::tracked_generated_c(&crust::compile(&renamed).unwrap(), false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "43\n97\n7\n8\n"
    );
}

#[test]
fn field_rename_preserves_destructor_cleanup_and_union_layout() {
    let source = r#"
struct Owner { Box<int> value; ~Owner() { print(this.value.get()); } };
union Bits { int value; unsigned char bytes[4]; };
int main() {
    Owner owner = { .value = Box<int>::create(42) };
    unsafe { Bits bits = { .value = 7 }; print(bits.value); }
    return 0;
}
"#;
    let analysis = crust::analyze(source).unwrap();
    let edits = analysis
        .rename(
            0,
            source.find("Box<int> value").unwrap() + "Box<int> ".len(),
            "storage",
        )
        .unwrap();
    let renamed = apply(&analysis, &edits).remove(0);
    assert!(renamed.contains("~Owner() { print(this.storage.get()); }"));
    assert!(renamed.contains(".storage = Box<int>::create(42)"));
    let analysis = crust::analyze(&renamed).unwrap();
    let edits = analysis
        .rename(0, renamed.find("int value;").unwrap() + 4, "number")
        .unwrap();
    let renamed = apply(&analysis, &edits).remove(0);
    assert!(renamed.contains("Bits bits = { .number = 7 }; print(bits.number)"));
    let output = common::tracked_generated_c(&crust::compile(&renamed).unwrap(), false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "7\n42\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        "drop:42\n"
    );
}

#[test]
fn enum_member_rename_updates_constants_scoped_uses_and_array_lengths() {
    let source = r#"
enum Flags : unsigned int { Read = 1, Write = 2, Both = (unsigned int)Flags::Read | (unsigned int)Write };
enum Length { Count = (unsigned int)Flags::Read + 2 };
int main() {
    Flags flags = Read;
    int values[(unsigned int)Flags::Read + 2] = { 1, 2, 3 };
    print((unsigned int)flags);
    print((unsigned int)Flags::Both);
    print(values[(int)Count - 1]);
    int Read = 7;
    print(Read);
    return 0;
}
"#;
    let analysis = crust::analyze(source).unwrap();
    let edits = analysis
        .rename(0, source.find("Read =").unwrap(), "Readable")
        .unwrap();
    let renamed = apply(&analysis, &edits).remove(0);
    assert!(renamed.contains("Readable = 1"));
    assert!(renamed.contains("Flags flags = Readable"));
    assert!(renamed.contains("int Read = 7;"));
    assert!(renamed.contains("print(Read)"));
    let output = common::tracked_generated_c(&crust::compile(&renamed).unwrap(), false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "1\n3\n3\n7\n"
    );
}

#[test]
fn member_rename_rejects_capture_unresolved_uses_and_native_enum_constants() {
    for (source, selected, name) in [
        (
            "struct Item { int value; int data; }; int main() { Item item = { .value = 1, .data = 2 }; return item.value; }",
            "value;",
            "data",
        ),
        (
            "struct Item { int value; }; int future<T>(T item) { return item.value; } int main() { return 0; }",
            "value;",
            "data",
        ),
        (
            "enum Flags { Read = 1, Write = 2 }; int main() { return (int)Read; }",
            "Read =",
            "Write",
        ),
        (
            "enum Flags { Read = 1 }; int main() { int Active = 7; return (int)Read; }",
            "Read =",
            "Active",
        ),
    ] {
        let analysis = crust::analyze(source).unwrap();
        assert!(
            analysis
                .rename(0, source.find(selected).unwrap(), name)
                .is_err(),
            "{source}"
        );
    }
    let source = "extern header \"native.h\"; extern enum NativeMode : int { On = 1 }; int main() { return (int)On; }";
    let analysis = crust::analyze(source).unwrap();
    assert!(
        analysis
            .rename_target(0, source.find("On =").unwrap())
            .is_none()
    );
}

#[test]
fn struct_rename_updates_forward_declarations_destructors_and_qualified_calls() {
    let source = r#"
struct Item;
typedef Item Alias;
struct Item {
    Box<int> value;
    ~Item() { print(this.value.get()); }
    static Self create(int value) { Self result = { .value = Box<int>::create(value) }; return move(result); }
    int read() const { return this.value.get(); }
};
int main() {
    Item first = Item::create(41);
    Alias second = Item::create(42);
    print(first.read());
    return 0;
}
"#;
    let analysis = crust::analyze(source).unwrap();
    assert!(
        analysis
            .rename_target(0, source.find("Self create").unwrap())
            .is_none()
    );
    let offset = source.find("~Item").unwrap() + 1;
    let edits = analysis.rename(0, offset, "Resource").unwrap();
    let renamed = apply(&analysis, &edits).remove(0);
    assert!(!renamed.contains("Item"));
    assert!(renamed.contains("struct Resource;"));
    assert!(renamed.contains("typedef Resource Alias;"));
    assert!(renamed.contains("~Resource()"));
    assert!(renamed.contains("static Self create"));
    assert!(renamed.contains("Resource first = Resource::create(41)"));
    let output = common::tracked_generated_c(&crust::compile(&renamed).unwrap(), false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "41\n42\n41\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        "drop:42\ndrop:41\n"
    );
}

#[test]
fn generic_struct_rename_updates_instantiations_and_preserves_self() {
    let source = r#"
struct Item<T> {
    T value;
    ~Item() { print(1); }
    static Self create(T value) { Self result = { .value = move(value) }; return move(result); }
    T read() const { return this.value; }
};
int main() {
    Item<int> first = Item<int>::create(42);
    Item<char> second = Item<char>::create('a');
    print(first.read());
    print(second.read());
    return 0;
}
"#;
    let analysis = crust::analyze(source).unwrap();
    let edits = analysis
        .rename(0, source.find("Item<int>").unwrap(), "Value")
        .unwrap();
    let renamed = apply(&analysis, &edits).remove(0);
    assert!(!renamed.contains("Item"));
    assert!(renamed.contains("struct Value<T>"));
    assert!(renamed.contains("~Value()"));
    assert!(renamed.contains("static Self create"));
    let output = common::tracked_generated_c(&crust::compile(&renamed).unwrap(), false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n97\n1\n1\n"
    );
}

#[test]
fn union_rename_updates_generic_and_ordinary_type_uses() {
    for (definition, ty) in [
        (
            "union Bits { unsigned int value; unsigned char bytes[4]; };",
            "Bits",
        ),
        (
            "union Bits<T> { T value; unsigned char bytes[4]; };",
            "Bits<unsigned int>",
        ),
    ] {
        let source = format!(
            "{definition} int main() {{ unsafe {{ {ty} bits = {{ .value = 42 }}; print(bits.value); print(sizeof({ty}) == sizeof(unsigned int)); }} return 0; }}"
        );
        let analysis = crust::analyze(&source).unwrap();
        let edits = analysis
            .rename(0, source.find("Bits").unwrap(), "Storage")
            .unwrap();
        let renamed = apply(&analysis, &edits).remove(0);
        assert!(!renamed.contains("Bits"));
        let output = common::tracked_generated_c(&crust::compile(&renamed).unwrap(), false);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
            "42\n1\n"
        );
    }
}

#[test]
fn aggregate_rename_preserves_native_tags_and_rejects_capture_or_unresolved_uses() {
    for source in [
        "struct Handle; int main() { return 0; }",
        "union Handle; int main() { return 0; }",
    ] {
        let analysis = crust::analyze(source).unwrap();
        let offset = source.find("Handle").unwrap();
        let edits = analysis.rename(0, offset, "Resource").unwrap();
        let renamed = apply(&analysis, &edits).remove(0);
        assert!(renamed.contains("Resource = \"Handle\";"));
        crust::check(&renamed).unwrap();
    }
    for source in [
        "struct Item { int value; }; struct Resource { int value; }; int main() { Item item = { .value = 1 }; return item.value; }",
        "struct Item { int value; }; T future<T>(T value) { Item item = { .value = 1 }; return value; } int main() { return 0; }",
    ] {
        let analysis = crust::analyze(source).unwrap();
        assert!(
            analysis
                .rename(0, source.find("Item").unwrap(), "Resource")
                .is_err()
        );
    }
}

#[test]
fn enum_rename_updates_qualifiers_type_expressions_and_constants() {
    let source = r#"
enum Flags : unsigned int { Read = 1, Write = 2, Both = (unsigned int)Flags::Read | (unsigned int)Flags::Write };
typedef Flags Mode;
struct Item { Flags value; };
T identity<T>(T value) { return value; }
int main() {
    Flags value = Flags::Both;
    Mode mode = identity<Flags>(value);
    Item item = { .value = mode };
    int values[(unsigned int)Flags::Both] = { 1, 2, 3 };
    print((unsigned int)item.value);
    print(values[2]);
    print(sizeof(Flags) == sizeof(unsigned int));
    int Flags = 7;
    print(Flags);
    return 0;
}
"#;
    let analysis = crust::analyze(source).unwrap();
    let edits = analysis
        .rename(0, source.find("Flags::Both").unwrap(), "Access")
        .unwrap();
    let renamed = apply(&analysis, &edits).remove(0);
    assert!(renamed.contains("enum Access : unsigned int"));
    assert!(renamed.contains("(unsigned int)Access::Read"));
    assert!(renamed.contains("identity<Access>"));
    assert!(renamed.contains("sizeof(Access)"));
    assert!(renamed.contains("int Flags = 7;"));
    assert!(renamed.contains("print(Flags)"));
    let output = common::tracked_generated_c(&crust::compile(&renamed).unwrap(), false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "3\n3\n1\n7\n"
    );
}

#[test]
fn opaque_tag_rename_keeps_native_identity_for_repeated_and_explicit_declarations() {
    let implicit = "extern struct Native* create(); int main() { return 0; }";
    let analysis = crust::analyze(implicit).unwrap();
    assert!(
        analysis
            .rename_target(0, implicit.find("Native").unwrap())
            .is_none()
    );
    for (kind, declaration) in [("struct", "Native"), ("union", "Local = \"Native\"")] {
        let local = declaration.split_whitespace().next().unwrap();
        let source = format!(
            "{kind} {declaration}; {kind} {local}; typedef {kind} {local} Handle; extern Handle* echo(Handle*) = \"native_echo\"; int main() {{ unsafe {{ Handle* pointer = 0; print(echo(pointer) == 0); }} return 0; }}"
        );
        let analysis = crust::analyze(&source).unwrap();
        let offset = source.find(declaration).unwrap();
        let edits = analysis.rename(0, offset, "Resource").unwrap();
        let renamed = apply(&analysis, &edits).remove(0);
        assert!(
            renamed.contains(&format!("{kind} Resource = \"Native\"; {kind} Resource;")),
            "{renamed}"
        );
        let generated = format!(
            "{}\n{kind} Native* native_echo({kind} Native* value) {{ return value; }}",
            crust::compile(&renamed).unwrap()
        );
        let output = common::tracked_generated_c(&generated, false);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
            "1\n"
        );
    }
}

#[test]
fn enum_rename_preserves_native_tags_with_implicit_and_explicit_aliases() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/enum-rename")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&root).unwrap();
    let header = root.join("native.h");
    std::fs::write(&header, "#ifndef ENUM_RENAME_HEADER\n#define ENUM_RENAME_HEADER\nenum NativeMode { Off = 0, On = 42 };\n#endif\n").unwrap();
    let header = header.to_string_lossy().replace('\\', "/");
    for declaration in ["NativeMode", "LocalMode = \"NativeMode\""] {
        let name = declaration.split_whitespace().next().unwrap();
        let source = format!(
            r#"
extern header "{header}";
extern enum {declaration} : int {{ Off = 0, On = 42 }};
extern {name} choose({name} value) {{ return value; }}
extern int native_check();
int main() {{ unsafe {{ print((int)choose({name}::On)); print(native_check()); }} return 0; }}
"#
        );
        let analysis = crust::analyze(&source).unwrap();
        let edits = analysis
            .rename(0, source.find(&format!("enum {name}")).unwrap() + 5, "Mode")
            .unwrap();
        let renamed = apply(&analysis, &edits).remove(0);
        assert!(
            renamed.contains("extern enum Mode = \"NativeMode\" : int"),
            "{renamed}"
        );
        let public_header = crust::emit_header(&renamed, crust::ProgramKind::Executable).unwrap();
        assert!(public_header.contains("enum NativeMode"));
        let generated = format!(
            "{}\nint32_t native_check(void) {{ enum NativeMode value = choose(On); return value == On; }}",
            crust::compile(&renamed).unwrap()
        );
        let output = common::tracked_generated_c(&generated, false);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
            "42\n1\n"
        );
    }
}

#[test]
fn enum_rename_rejects_type_capture_and_unresolved_template_references() {
    for source in [
        "enum Mode { On }; typedef int State; int main() { Mode mode = On; State value = 1; return value; }",
        "enum Mode { On }; T unused<T>() { Mode value = On; return value; } int main() { return 0; }",
    ] {
        let analysis = crust::analyze(source).unwrap();
        assert!(
            analysis
                .rename(0, source.find("Mode").unwrap(), "State")
                .is_err()
        );
    }
}

#[test]
fn type_parameter_rename_covers_struct_members_and_separate_instantiations() {
    let source = r#"
struct Pair<T, U> {
    T first;
    U second;
    T read() const { return this.first; }
    static Self create(T first, U second) { Self result = { .first = first, .second = second }; return result; }
};
T echo<T>(T value) { return value; }
int main() {
    Pair<int, char> first = Pair<int, char>::create(42, 'a');
    Pair<char, int> second = Pair<char, int>::create('b', 7);
    print(first.read());
    print(second.read());
    print(echo<int>(8));
    return 0;
}
"#;
    let analysis = crust::analyze(source).unwrap();
    let edits = analysis
        .rename(0, source.find("T, U").unwrap(), "Element")
        .unwrap();
    let renamed = apply(&analysis, &edits).remove(0);
    assert!(renamed.contains("struct Pair<Element, U>"));
    assert!(renamed.contains("Element first;"));
    assert!(renamed.contains("Element read() const"));
    assert!(renamed.contains("create(Element first, U second)"));
    assert!(renamed.contains("T echo<T>(T value)"));
    let output = common::tracked_generated_c(&crust::compile(&renamed).unwrap(), false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n98\n8\n"
    );
}

#[test]
fn type_parameter_rename_preserves_generic_owner_cleanup_and_union_access() {
    let source = r#"
struct Holder<T> { T value; };
union Bits<T> { T value; unsigned char byte; };
int main() {
    Holder<Box<int>> holder = { .value = Box<int>::create(42) };
    print(holder.value.get());
    unsafe { Bits<int> bits = { .value = 7 }; print(bits.value); }
    return 0;
}
"#;
    let analysis = crust::analyze(source).unwrap();
    let edits = analysis
        .rename(0, source.find("T>").unwrap(), "Value")
        .unwrap();
    let renamed = apply(&analysis, &edits).remove(0);
    assert!(renamed.contains("Holder<Value> { Value value; }"));
    assert!(renamed.contains("union Bits<T> { T value;"));
    let analysis = crust::analyze(&renamed).unwrap();
    let edits = analysis
        .rename(
            0,
            renamed.find("Bits<T>").unwrap() + "Bits<".len(),
            "Storage",
        )
        .unwrap();
    let renamed = apply(&analysis, &edits).remove(0);
    assert!(renamed.contains("union Bits<Storage> { Storage value;"));
    let output = common::tracked_generated_c(&crust::compile(&renamed).unwrap(), false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n7\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        "drop:42\n"
    );
}

#[test]
fn type_parameter_rename_rejects_capture_and_unresolved_template_uses() {
    for (source, name) in [
        (
            "T echo<T, U>(T value) { return value; } int main() { return echo<int, char>(1); }",
            "U",
        ),
        (
            "typedef int Item; T echo<T>(T value) { Item other = 1; return value; } int main() { return echo<int>(1); }",
            "Item",
        ),
        (
            "T echo<T>(T value) { return value; } int main() { return 0; }",
            "Element",
        ),
    ] {
        let analysis = crust::analyze(source).unwrap();
        let offset = source.find("echo<T").unwrap() + "echo<".len();
        assert!(analysis.rename(0, offset, name).is_err(), "{source}");
    }
}

#[test]
fn reanalyzes_imports_from_the_snapshot_with_unicode_and_no_disk_writes() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/virtual-rename");
    let entry = "import \"helper.cr\"; int main() { return answer(42); }";
    let helper = "// 🦀\r\nint answer(int input) { return (input); }";
    let sources = crust::source::SourceSet::load_with(&root.join("main.cr"), |path| {
        return Ok(if path.ends_with("main.cr") {
            entry.to_owned()
        } else {
            helper.to_owned()
        });
    })
    .unwrap();
    let analysis = sources.analyze().unwrap();
    let offset = helper.find("input);").unwrap();
    let (_, range) = analysis.rename_target(1, offset).unwrap();
    assert_eq!(&helper[range.span.start..range.span.end], "input");
    let edits = analysis.rename(1, offset, "n").unwrap();
    assert_eq!(edits.len(), 2);
    assert!(edits.iter().all(|edit| edit.location.file == 1));
    let result = apply(&analysis, &edits);
    assert_eq!(result[0], entry);
    assert_eq!(result[1], "// 🦀\r\nint answer(int n) { return (n); }");
}

#[test]
fn renames_functions_calls_recursion_and_addresses_without_touching_shadowing_parameters() {
    let source = "typedef int Callback(int); int helper(int value) { if (value == 0) { return 0; } return helper(value - 1); } int other(int helper) { return helper; } int main() { Callback* function = helper; unsafe { print(function(2)); } return helper(1); }";
    let analysis = crust::analyze(source).unwrap();
    let edits = analysis
        .rename(0, source.find("helper(int").unwrap(), "calculate")
        .unwrap();
    let result = apply(&analysis, &edits).remove(0);
    assert!(result.contains("int calculate(int value)"));
    assert!(result.contains("return calculate(value - 1)"));
    assert!(result.contains("function = calculate"));
    assert!(result.contains("int other(int helper) { return helper; }"));
    crust::check(&result).unwrap();
}

#[test]
fn native_function_and_global_renames_keep_c_symbols_and_existing_aliases() {
    let source = "extern int native(int); extern int native(int input) = \"native\"; extern const int native_value; extern const int alias = \"native_value\"; int main() { unsafe { print(native(native_value)); print(alias); } return 0; }";
    for (selected, name) in [("native(int)", "call_native"), ("native_value;", "v")] {
        let analysis = crust::analyze(source).unwrap();
        let edits = analysis
            .rename(0, source.find(selected).unwrap(), name)
            .unwrap();
        assert!(
            edits
                .iter()
                .any(|edit| edit.location.span.start == edit.location.span.end)
        );
        let result = apply(&analysis, &edits).remove(0);
        if name == "v" {
            assert!(result.contains("extern const int v = \"native_value\";"));
        } else {
            assert!(result.contains("extern int call_native(int) = \"native\";"));
        }
        assert!(result.contains("extern const int alias = \"native_value\";"));
        let mut c = crust::compile(&result).unwrap();
        c.push_str("int native(int value) { return value + 1; } const int native_value = 41;");
        let output = common::tracked_generated_c(&c, false);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
            "42\n41\n"
        );
    }
}

#[test]
fn global_rename_rejects_callable_capture_and_unresolved_template_occurrences() {
    let source = "typedef int Callback(int); int helper(int value) { return value; } int other(int value) { return value + 1; } int main() { Callback* target = other; unsafe { return helper(1); } }";
    let analysis = crust::analyze(source).unwrap();
    let error = analysis
        .rename(0, source.find("helper(int").unwrap(), "target")
        .unwrap_err();
    assert!(error.contains("which declaration"), "{error}");
    let source = "int helper(int value) { return value; } T future<T>(T value) { return helper(value); } int main() { return helper(1); }";
    let analysis = crust::analyze(source).unwrap();
    assert!(
        analysis
            .rename(0, source.find("helper(int").unwrap(), "calculate")
            .unwrap_err()
            .contains("cannot resolve")
    );
    let source = "T future<T>(T value) { return value; } int main() { return 0; }";
    let analysis = crust::analyze(source).unwrap();
    assert!(
        analysis
            .rename(0, source.find("value)").unwrap(), "input")
            .is_err()
    );
}

#[test]
fn generic_function_rename_updates_every_instantiation_and_keeps_receiver_names_fixed() {
    let source = "T identity<T>(T value) { return value; } int main() { print(identity<int>(1)); print(identity<char>('a')); return 0; }";
    let analysis = crust::analyze(source).unwrap();
    let edits = analysis
        .rename(0, source.find("identity<T>").unwrap(), "echo")
        .unwrap();
    let result = apply(&analysis, &edits).remove(0);
    assert!(result.contains("T echo<T>"));
    assert!(result.contains("echo<int>(1)"));
    assert!(result.contains("echo<char>('a')"));
    let source = "struct Item { int value; int take(Self this) { return this.value; } }; int main() { return 0; }";
    let analysis = crust::analyze(source).unwrap();
    assert!(
        analysis
            .rename_target(0, source.find("this)").unwrap())
            .is_none()
    );
}

#[test]
fn function_rename_updates_imports_using_snapshot_text() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/virtual-function-rename");
    let entry = "import \"helper.cr\"; int main() { return answer(42); }";
    let helper = "// 🦀\r\nint answer(int input) { return input; }";
    let sources = crust::source::SourceSet::load_with(&root.join("main.cr"), |path| {
        return Ok(if path.ends_with("main.cr") {
            entry.to_owned()
        } else {
            helper.to_owned()
        });
    })
    .unwrap();
    let analysis = sources.analyze().unwrap();
    let edits = analysis
        .rename(0, entry.find("answer(42)").unwrap(), "calculate")
        .unwrap();
    let result = apply(&analysis, &edits);
    assert_eq!(
        result[0],
        "import \"helper.cr\"; int main() { return calculate(42); }"
    );
    assert_eq!(
        result[1],
        "// 🦀\r\nint calculate(int input) { return input; }"
    );
}

#[test]
fn function_rename_keeps_independent_qualified_members_in_unused_templates() {
    let source = "int helper() { return 1; } interface Uses { int helper() const; }; int later<T: Uses>(const T& input) { return input.helper(); } int main() { return helper(); }";
    let analysis = crust::analyze(source).unwrap();
    let edits = analysis
        .rename(0, source.find("helper()").unwrap(), "compute")
        .unwrap();
    let result = apply(&analysis, &edits).remove(0);
    assert!(result.contains("int compute()"));
    assert!(result.contains("return compute();"));
    assert!(result.contains("return input.helper();"));
}
