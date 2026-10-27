mod common;

use std::collections::HashMap;
use std::io;
use std::path::Path;

use crust::source::{SourceSet, identity};

#[test]
fn static_helpers_resolve_in_their_definition_file_and_execute() {
    let sources = load(&[
        ("main.cr", "import \"a.cr\"; import \"b.cr\"; int helper() { return 100; } int main() { print(a()); print(b()); print(wrapped<int>(7)); print(helper()); return 0; }"),
        ("a.cr", "typedef int (*Callback)(int); int a() { Callback call = &helper; unsafe { return call(3); } } static int helper(int x) { if (x == 0) return 10; return helper(x - 1) + 1; } T wrapped<T>(T value) { helper(0); return value; }"),
        ("b.cr", "int b() { return helper<int>(20); } static T helper<T>(T value) { return value; }"),
    ]).unwrap();
    let output = common::tracked_generated_c(&sources.compile().unwrap(), false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "13\n20\n7\n100\n"
    );
}

#[test]
fn static_helpers_are_hidden_from_direct_and_transitive_importers() {
    for call in ["helper()", "helper<int>(1)", "&helper"] {
        let main = format!("import \"middle.cr\"; int main() {{ {call}; return 0; }}");
        let sources = load(&[
            ("main.cr", &main),
            ("middle.cr", "import \"helper.cr\";"),
            (
                "helper.cr",
                if call.contains('<') {
                    "static T helper<T>(T value) { return value; }"
                } else {
                    "static int helper() { return 1; }"
                },
            ),
        ])
        .unwrap();
        assert!(sources.check().is_err(), "accepted {call}");
    }
}

#[test]
fn static_functions_reject_incompatible_linkage_and_duplicate_definitions() {
    for text in [
        "static int main() { return 0; }",
        "static extern int helper(); int main() { return 0; }",
        "static int helper() { return 1; } int helper() { return 2; } int main() { return 0; }",
        "static int helper() { return 1; } static int helper() { return 2; } int main() { return 0; }",
    ] {
        assert!(
            load(&[("main.cr", text)]).unwrap().check().is_err(),
            "accepted {text}"
        );
    }
}

#[test]
fn static_helpers_respect_local_shadowing_and_editor_visibility() {
    let main = "import \"helper.cr\"; int main() { return visible(); }";
    let helper =
        "static int hidden() { return 42; } int visible() { int hidden = 7; return hidden; }";
    let sources = load(&[("main.cr", main), ("helper.cr", helper)]).unwrap();
    sources.check().unwrap();
    let analysis = sources.analyze().unwrap();
    let main_file = analysis
        .files
        .iter()
        .position(|file| file.text == main)
        .unwrap();
    let helper_file = analysis
        .files
        .iter()
        .position(|file| file.text == helper)
        .unwrap();
    assert!(
        !analysis
            .completions_at(main_file, main.find("return").unwrap())
            .iter()
            .any(|symbol| symbol.name == "hidden")
    );
    assert!(
        analysis
            .completions_at(helper_file, helper.find("return").unwrap())
            .iter()
            .any(|symbol| symbol.name == "hidden")
    );
}

#[test]
fn native_headers_resolve_from_imported_sources_and_are_loaded_once() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/native-header-imports")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(root.join("native")).unwrap();
    std::fs::create_dir_all(root.join("lib")).unwrap();
    std::fs::write(
        root.join("native/native ü.h"),
        "enum Status { Ready = 42, Failed = -1 }; extern const enum Status native_status;",
    )
    .unwrap();
    std::fs::write(root.join("lib/bindings.cr"), "extern header \"../native/native ü.h\"; extern header \"../native/./native ü.h\"; extern enum Status : int { Ready = 42, Failed = -1 }; extern const Status native_status;").unwrap();
    std::fs::write(root.join("main.cr"), "import \"lib/bindings.cr\"; int main() { unsafe { print((int)native_status); } return 0; }").unwrap();
    let sources = SourceSet::load(&root.join("main.cr")).unwrap();
    assert_eq!(sources.native_headers().len(), 1);
    assert_eq!(
        sources.native_headers()[0],
        root.join("native/native ü.h").canonicalize().unwrap()
    );
    let mut c = sources.compile().unwrap();
    c.push_str("const enum Status native_status = Ready;");
    let output = common::tracked_generated_c(&c, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");
}

#[test]
fn missing_native_headers_report_the_crust_file_and_reference_location() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/virtual-missing-native-header");
    let error = SourceSet::load_with(&root.join("main.cr"), |_| {
        Ok("// comment\nextern header \"missing.h\"; int main() { return 0; }".to_owned())
    })
    .err()
    .unwrap();
    assert!(error.path.ends_with("main.cr"));
    assert_eq!(error.diagnostic.span.line, 2);
    assert!(error.diagnostic.message.contains("native header"));
}

fn load(files: &[(&str, &str)]) -> Result<SourceSet, crust::source::SourceError> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/virtual-imports");
    let files: HashMap<_, _> = files
        .iter()
        .map(|(name, text)| (identity(&root.join(name)).unwrap(), text.to_string()))
        .collect();
    return SourceSet::load_with(&root.join("main.cr"), |path| {
        files
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    });
}

#[test]
fn imports_are_relative_transitive_and_loaded_once_through_diamonds_and_cycles() {
    let sources = load(&[
        (
            "main.cr",
            "import \"lib/a.cr\"; import \"lib/b.cr\"; int main() { return a() + b(); }",
        ),
        (
            "lib/a.cr",
            "import \"shared.cr\"; import \"../main.cr\"; int a() { return shared(); }",
        ),
        (
            "lib/b.cr",
            "import \"../lib/shared.cr\"; int b() { return shared(); }",
        ),
        ("lib/shared.cr", "int shared() { return 21; }"),
    ])
    .unwrap();
    assert_eq!(sources.files().len(), 4);
    sources.check().unwrap();
    sources.compile().unwrap();
}

#[test]
fn imported_generic_types_functions_interfaces_and_global_aliases_resolve_forward() {
    load(&[
        ("main.cr", "import \"container.cr\"; import \"factory.cr\"; typedef Holder<int> Value; int main() { Value item = make<Value>(42); return item.read(); }"),
        ("container.cr", "struct Holder<T> { T value; static Self create(T value) { return (Self){ .value = value }; } int read() const { return this.value; } };"),
        ("factory.cr", "static interface Factory { Self create(int value); }; T make<T: Factory>(int value) { return T::create(value); }"),
    ]).unwrap().check().unwrap();
}

#[test]
fn frontend_errors_retain_the_imported_filename_local_offsets_and_unicode_columns() {
    let text = "// λ\nint helper() { return missing; }";
    let sources = load(&[
        (
            "main.cr",
            "import \"helper.cr\"; int main() { return helper(); }",
        ),
        ("helper.cr", text),
    ])
    .unwrap();
    let error = sources.check().unwrap_err();
    assert!(error.path.ends_with("helper.cr"));
    assert_eq!(error.diagnostic.span.start, text.find("missing").unwrap());
    assert_eq!(error.diagnostic.span.line, 2);
    assert_eq!(error.diagnostic.span.column, 23);
    assert!(error.to_string().contains("helper.cr:2:23:"));

    let error = match load(&[
        ("main.cr", "import \"helper.cr\"; int main() { return 0; }"),
        ("helper.cr", "// λ\n@"),
    ]) {
        Ok(_) => panic!("expected a lexer error"),
        Err(error) => error,
    };
    assert!(error.path.ends_with("helper.cr"));
    assert_eq!(error.diagnostic.span.start, 6);
}

#[test]
fn missing_and_malformed_imports_report_the_import_site() {
    for source in [
        "import \"missing.cr\";",
        "import 42;",
        "import \"\";",
        "import \"x\\0y\";",
        "import \"helper.cr\"",
        "import \"/absolute.cr\";",
    ] {
        let error = match load(&[("main.cr", source)]) {
            Ok(_) => panic!("should reject {source}"),
            Err(error) => error,
        };
        assert!(error.path.ends_with("main.cr"));
        assert_eq!(error.diagnostic.span.start, 0);
    }
}

#[test]
fn source_boundaries_cannot_complete_unfinished_declarations_or_bodies() {
    for source in [
        "import \"helper.cr\"; int",
        "import \"helper.cr\"; int main() {",
        "import \"helper.cr\"; struct S { int member;",
        "import \"helper.cr\"; T f<T>(T value) {",
    ] {
        let sources = load(&[("main.cr", source), ("helper.cr", "main() { return 0; }")]).unwrap();
        assert!(sources.check().is_err(), "should reject {source}");
    }
}

#[test]
fn multi_file_generic_container_runs_and_destroys_all_allocations() {
    let entry =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/container_project/src/main.cr");
    let sources = SourceSet::load(&entry).unwrap();
    let output = common::tracked_generated_c(&sources.compile().unwrap(), true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "10\n20\n98\n3\n4\n42\n3\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        format!(
            "drop:bytes:{}\ndrop:bytes:4\ndrop:bytes:4\ndrop:bytes:4\ndrop:bytes:4\ndrop:bytes:{}\ndrop:bytes:8\ndrop:bytes:{}\ndrop:bytes:16\ndrop:bytes:3\ndrop:bytes:12\n",
            std::mem::size_of::<usize>() * 2,
            std::mem::size_of::<usize>() * 4,
            std::mem::size_of::<usize>() * 2
        )
    );
}

#[test]
fn import_recursion_is_bounded_and_absolute_paths_are_rejected_before_reading() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/deep-import-test/main.cr");
    let mut reads = 0;
    let result = SourceSet::load_with(&root, |_| {
        reads += 1;
        return Ok(format!("import \"file{reads}.cr\";"));
    });
    let error = match result {
        Ok(_) => panic!("expected an import-depth error"),
        Err(error) => error,
    };
    assert!(error.diagnostic.message.contains("import limit"));
    assert_eq!(reads, 128);
    let error = match load(&[("main.cr", "import \"/absolute.cr\";")]) {
        Ok(_) => panic!("expected a relative-path error"),
        Err(error) => error,
    };
    assert!(error.diagnostic.message.contains("must be relative"));
    assert!(
        load(&[
            ("main.cr", "int import \"helper.cr\"; main() { return 0; }"),
            ("helper.cr", "")
        ])
        .is_err()
    );
}
