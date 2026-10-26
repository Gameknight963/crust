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
