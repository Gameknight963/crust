use crust::source::SourceSet;
use std::path::Path;
mod common;

fn error(generated: &str) -> String {
    let output = common::tracked_generated_c(generated, false);
    assert_eq!(output.status.code(), Some(1));
    return String::from_utf8(output.stderr)
        .unwrap()
        .replace("\r\n", "\n");
}

#[test]
fn imported_runtime_failures_report_the_original_filename_line_and_column() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/virtual-runtime-locations");
    for (body, needle, message) in [
        (
            "return 2147483647 + 1;",
            "2147483647 + 1",
            "integer overflow",
        ),
        (
            "int values[1] = { 1 }; int index = 1; return values[index];",
            "index];",
            "array index out of bounds",
        ),
        (
            "int64_t value = 300; return (uint8_t)value;",
            "value;",
            "integer conversion out of range",
        ),
        ("return 1 << 32;", "1 << 32", "shift count out of range"),
        (
            "uint64_t value = 1; return (int)(value / 0);",
            "(value / 0)",
            "division by zero",
        ),
        (
            "int value = -2147483648; return -value;",
            "-value",
            "integer overflow",
        ),
    ] {
        let helper = format!("// 🦀\nint fail() {{\n    {body}\n}}\n");
        let sources = SourceSet::load_with(&root.join("main.cr"), |path| {
            return Ok(if path.ends_with("main.cr") {
                "import \"checks.cr\"; int main() { return fail(); }".to_owned()
            } else {
                helper.clone()
            });
        })
        .unwrap();
        let output = error(&sources.compile().unwrap());
        let offset = helper.find(needle).unwrap();
        let line = helper[..offset]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1;
        let column = helper[..offset]
            .rsplit('\n')
            .next()
            .unwrap()
            .chars()
            .count()
            + 1;
        assert!(
            output.contains(&format!(
                "checks.cr:{line}:{column}: runtime error: {message}"
            )),
            "{output}"
        );
        assert!(!output.contains("main.cr:"), "{output}");
    }
}

#[test]
fn generic_runtime_failures_use_the_template_source_location() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/virtual-runtime-generics");
    let helper = "T next<T>(T value) {\n    return value + 1;\n}";
    let sources = SourceSet::load_with(&root.join("main.cr"), |path| {
        return Ok(if path.ends_with("main.cr") {
            "import \"generic.cr\"; int main() { print(next<int64_t>(9223372036854775807)); return 0; }".to_owned()
        } else { helper.to_owned() });
    }).unwrap();
    let output = error(&sources.compile().unwrap());
    assert!(
        output.contains("generic.cr:2:12: runtime error: integer overflow"),
        "{output}"
    );
}

#[test]
fn runtime_filenames_round_trip_quotes_trigraphs_spaces_and_unicode() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("target/virtual-runtime-paths/quoted\"??-🦀.cr");
    let sources =
        SourceSet::load_with(&path, |_| Ok("int main() { return 1 / 0; }".to_owned())).unwrap();
    let output = error(&sources.compile().unwrap());
    assert!(
        output.contains("quoted\"??-🦀.cr:1:21: runtime error: division by zero"),
        "{output}"
    );
}

#[test]
fn string_compilation_keeps_runtime_line_and_column_diagnostics() {
    let output = error(&crust::compile("int main() { return 1 / 0; }").unwrap());
    assert_eq!(output, "runtime error at 1:21: division by zero\n");
}

#[test]
fn c_callers_of_library_exports_report_the_crust_source_file() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("target/virtual-runtime-library/library.cr");
    let sources = SourceSet::load_with(&path, |_| {
        Ok("extern int increment(int value) {\n    return value + 1;\n}".to_owned())
    })
    .unwrap();
    let generated = format!(
        "{}\nint main(void) {{ return increment(INT32_MAX); }}",
        sources
            .compile_with_kind(crust::ProgramKind::Library)
            .unwrap()
    );
    let output = error(&generated);
    assert!(
        output.contains("library.cr:2:12: runtime error: integer overflow"),
        "{output}"
    );
}
