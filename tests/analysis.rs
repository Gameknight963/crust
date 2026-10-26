use crust::analysis::{Analysis, SymbolKind};

fn definition<'a>(
    analysis: &'a Analysis,
    source: &str,
    needle: &str,
) -> &'a crust::analysis::Symbol {
    let offset = source.find(needle).unwrap();
    let definitions = analysis.definitions_at(0, offset);
    assert_eq!(definitions.len(), 1, "expected a definition at {needle}");
    return definitions[0];
}

#[test]
fn bindings_resolve_parameters_shadowed_locals_reads_writes_and_calls() {
    let source = r#"
        int answer(int input) {
            int value = input;
            { int value = 7; print(value); }
            value = 42;
            return value;
        }
        int main() { int input = 1; return answer(input); }
    "#;
    let analysis = crust::analyze(source).unwrap();
    assert_eq!(
        definition(&analysis, source, "input;").kind,
        SymbolKind::Parameter
    );
    assert_eq!(
        definition(&analysis, source, "input;").location.span.start,
        source.find("input)").unwrap()
    );
    assert_eq!(
        definition(&analysis, source, "value);").location.span.start,
        source.find("value = 7").unwrap()
    );
    assert_eq!(
        definition(&analysis, source, "value = 42")
            .location
            .span
            .start,
        source.find("value = input").unwrap()
    );
    assert_eq!(
        definition(&analysis, source, "value;").location.span.start,
        source.find("value = input").unwrap()
    );
    assert_eq!(
        definition(&analysis, source, "answer(input)").detail,
        "int answer(int input)"
    );
    assert_eq!(
        definition(&analysis, source, "input);").location.span.start,
        source.find("input = 1").unwrap()
    );
}

#[test]
fn fields_methods_static_factories_enums_and_callbacks_keep_their_resolved_declarations() {
    let source = r#"
        enum Status { Ready = 7 };
        typedef int (*Callback)(int);
        struct Counter {
            int value;
            static Self create(int value) { return (Self){ .value = value }; }
            int get() const { return this->value; }
        };
        int identity(int value) { return value; }
        int main() {
            Counter counter = Counter::create(42);
            print(counter.get()); print(counter.value);
            print((int)Status::Ready);
            Callback callback = identity; unsafe { print(callback(1)); }
            return 0;
        }
    "#;
    let analysis = crust::analyze(source).unwrap();
    assert_eq!(
        definition(&analysis, source, "get());").detail,
        "int get() const"
    );
    assert_eq!(
        definition(&analysis, source, "create(42)").detail,
        "static Self create(int value)"
    );
    assert_eq!(
        definition(&analysis, source, "value);").kind,
        SymbolKind::Field
    );
    assert_eq!(
        definition(&analysis, source, "Ready);").kind,
        SymbolKind::EnumMember
    );
    assert_eq!(
        definition(&analysis, source, "identity;")
            .location
            .span
            .start,
        source.find("identity(int").unwrap()
    );
    assert_eq!(
        definition(&analysis, source, "callback(1)").kind,
        SymbolKind::Variable
    );
    assert_eq!(
        definition(&analysis, source, "callback(1)").detail,
        "Callback callback"
    );
}

#[test]
fn instantiated_generics_share_source_declarations_and_keep_call_site_types() {
    let source = "T identity<T>(T value) { return value; } int main() { print(identity<int>(42)); print(identity<char>('a')); return 0; }";
    let analysis = crust::analyze(source).unwrap();
    let first = definition(&analysis, source, "identity<int>");
    let second = definition(&analysis, source, "identity<char>");
    assert_eq!(first.location, second.location);
    assert_eq!(first.detail, "T identity<T>(T value)");
    assert_eq!(
        analysis
            .type_at(0, source.find("42").unwrap())
            .unwrap()
            .description,
        "int"
    );
    assert_eq!(
        analysis
            .type_at(0, source.find("'a'").unwrap())
            .unwrap()
            .description,
        "char"
    );
    assert_eq!(definition(&analysis, source, "value;").detail, "T value");
}

#[test]
fn analysis_uses_the_same_frontend_errors_as_compilation() {
    let source = "int main() { BoxInt owner = box_int(1); BoxInt moved = move(owner); print(*owner); return 0; }";
    assert_eq!(
        crust::analyze(source).unwrap_err(),
        crust::check(source).unwrap_err()
    );
}

#[test]
fn analysis_maps_imported_declarations_to_their_own_file_and_byte_offsets() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/virtual-analysis");
    let entry = "import \"helper.cr\"; int main() { return answer(42); }";
    let helper = "// 🦀\r\nint answer(int value) { return value; }";
    let sources = crust::source::SourceSet::load_with(&root.join("main.cr"), |path| {
        return Ok(if path.ends_with("main.cr") {
            entry.to_owned()
        } else {
            helper.to_owned()
        });
    })
    .unwrap();
    let analysis = sources.analyze().unwrap();
    let definitions = analysis.definitions_at(0, entry.find("answer").unwrap());
    assert_eq!(definitions.len(), 1);
    let location = definitions[0].location;
    assert_eq!(location.file, 1);
    assert_eq!(location.span.start, helper.find("answer").unwrap());
    assert_eq!(location.span.line, 2);
    assert!(
        analysis.files[location.file]
            .path
            .as_ref()
            .unwrap()
            .ends_with("helper.cr")
    );
    assert_eq!(definitions[0].detail, "int answer(int value)");
}

#[test]
fn type_names_follow_alias_precedence_explicit_tags_and_scoped_enum_members() {
    let source = r#"
        struct Item { int value; };
        typedef int Item;
        enum Status { Ready };
        union Handle;
        int main() {
            Item count = 1;
            struct Item record = { .value = 2 };
            Status status = Status::Ready;
            union Handle* pointer = 0;
            return count + record.value;
        }
    "#;
    let analysis = crust::analyze(source).unwrap();
    assert_eq!(
        definition(&analysis, source, "Item count").kind,
        SymbolKind::Alias
    );
    assert_eq!(
        definition(&analysis, source, "Item count").detail,
        "typedef int Item"
    );
    assert_eq!(
        definition(&analysis, source, "Item record").kind,
        SymbolKind::Struct
    );
    assert_eq!(
        definition(&analysis, source, "Status::Ready").kind,
        SymbolKind::Enum
    );
    assert_eq!(
        definition(&analysis, source, "Ready;").kind,
        SymbolKind::EnumMember
    );
    assert_eq!(
        definition(&analysis, source, "Handle* pointer").kind,
        SymbolKind::Union
    );
}

#[test]
fn unused_templates_interfaces_constraints_and_generic_parameter_bindings_have_symbols() {
    let source = r#"
        interface Readable { int read() const; };
        struct Unused<T: Readable> { T value; };
        T identity<T>(T T) { return T; }
        int main() { print(identity<int>(42)); return 0; }
    "#;
    let analysis = crust::analyze(source).unwrap();
    assert!(
        analysis
            .symbols
            .iter()
            .any(|symbol| symbol.name == "Unused" && symbol.kind == SymbolKind::Struct)
    );
    assert!(analysis.symbols.iter().any(|symbol| symbol.name == "read"
        && symbol.kind == SymbolKind::Method
        && symbol.container.as_deref() == Some("Readable")));
    assert_eq!(
        definition(&analysis, source, "Readable>").kind,
        SymbolKind::Interface
    );
    assert_eq!(
        definition(&analysis, source, "T identity").kind,
        SymbolKind::TypeParameter
    );
    assert_eq!(
        definition(&analysis, source, "T T)").kind,
        SymbolKind::TypeParameter
    );
    assert_eq!(
        definition(&analysis, source, "T;").kind,
        SymbolKind::Parameter
    );
    assert_eq!(definition(&analysis, source, "T;").detail, "T T");
}

#[test]
fn type_arguments_and_qualified_type_names_take_precedence_over_enclosing_function_references() {
    let source = r#"
        typedef int Count;
        typedef int (*Callback)(int);
        T identity<T>(T value) { return value; }
        struct Helpers { int value; static int read(int value) { return value; } };
        int main() { Callback first = identity<int>; Callback second = identity<Count>; Callback third = Helpers::read; return 0; }
    "#;
    let analysis = crust::analyze(source).unwrap();
    assert!(
        analysis
            .definitions_at(0, source.find("int>;").unwrap())
            .is_empty()
    );
    assert_eq!(
        analysis
            .type_at(0, source.find("int>;").unwrap())
            .unwrap()
            .description,
        "int"
    );
    assert_eq!(
        definition(&analysis, source, "Count>;").kind,
        SymbolKind::Alias
    );
    assert_eq!(
        definition(&analysis, source, "Helpers::read").kind,
        SymbolKind::Struct
    );
    assert_eq!(
        definition(&analysis, source, "read;").kind,
        SymbolKind::Method
    );
}

#[test]
fn unused_interface_signatures_keep_self_and_aliases_in_their_declaration_scope() {
    let source = "typedef int Count; interface Factory { static Self create(Count count); }; int main() { return 0; }";
    let analysis = crust::analyze(source).unwrap();
    assert_eq!(
        definition(&analysis, source, "Self create").kind,
        SymbolKind::Interface
    );
    assert_eq!(
        definition(&analysis, source, "Count count").kind,
        SymbolKind::Alias
    );
    assert_eq!(
        definition(&analysis, source, "create(Count").detail,
        "static Self create(Count count)"
    );
}

#[test]
fn reference_ranges_preserve_identifier_tokens_and_implicit_receivers_are_not_method_uses() {
    let source = r#"
        typedef int (*Callback)(int);
        int identity(int value) { return value; }
        struct Counter { int value; int get() const { return (this)->value; } };
        int main() {
            int number = 7; print((number));
            Callback call = &(identity); unsafe { print(call(1)); }
            Counter counter = { .value = 42 }; print(counter.get()); return 0;
        }
    "#;
    let analysis = crust::analyze(source).unwrap();
    for needle in ["number));", "identity);", "call(1)"] {
        let offset = source.find(needle).unwrap();
        let reference = analysis
            .references
            .iter()
            .find(|reference| {
                reference.location.span.start == offset && reference.declaration.is_some()
            })
            .unwrap();
        let text = &source[reference.location.span.start..reference.location.span.end];
        assert!(
            text.chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_'),
            "{text}"
        );
    }
    let method = analysis
        .symbols
        .iter()
        .find(|symbol| symbol.name == "get")
        .unwrap();
    assert_eq!(
        analysis
            .references
            .iter()
            .filter(|reference| reference.declaration == Some(method.location))
            .count(),
        1
    );
    let offset = source.find("this").unwrap();
    assert!(analysis.definitions_at(0, offset).is_empty());
    assert_eq!(
        analysis.type_at(0, offset).unwrap().description,
        "const struct Counter^"
    );
}
