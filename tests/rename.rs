use crust::analysis::{Analysis, TextEdit};

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
    assert!(
        analysis
            .rename_target(0, source.find("T>(T").unwrap())
            .is_none()
    );
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
