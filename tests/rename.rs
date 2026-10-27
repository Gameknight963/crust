use crust::analysis::{Analysis, TextEdit};
mod common;

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
    let source = "int helper() { return 1; } interface Uses { int helper() const; }; int later<T: Uses>(const T^ input) { return input->helper(); } int main() { return helper(); }";
    let analysis = crust::analyze(source).unwrap();
    let edits = analysis
        .rename(0, source.find("helper()").unwrap(), "compute")
        .unwrap();
    let result = apply(&analysis, &edits).remove(0);
    assert!(result.contains("int compute()"));
    assert!(result.contains("return compute();"));
    assert!(result.contains("return input->helper();"));
}
