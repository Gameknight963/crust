mod common;

#[test]
fn source_names_preserve_shadowed_owner_cleanup_and_c_namespace_collisions() {
    let output = common::tracked_run(
        r#"
struct abs { int magnitude; };
int inspect(int value) {
    BoxInt owner = box_int(value);
    {
        BoxInt owner = box_int(value + 1);
        int printf = 3;
        int int32_t = 4;
        int auto = 5;
        abs magnitude = { .magnitude = auto };
        print(printf + int32_t + magnitude.magnitude);
        return *owner;
    }
}
int main() { print(inspect(41)); return 0; }
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "12\n42\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace('\r', ""),
        "drop:42\ndrop:41\n"
    );
}

#[test]
fn native_headers_keep_their_macros_typedefs_and_tags_separate_from_source_names() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/debug-name-collisions")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("names.h"),
        "#define score 123\ntypedef int Point;\nstruct Point { double native_field; };\n",
    )
    .unwrap();
    let entry = directory.join("main.cr");
    std::fs::write(
        &entry,
        r#"
extern header "names.h";
struct Point { int score; };
int inspect(int score) { Point point = { .score = score }; return point.score; }
int main() { print(inspect(42)); return 0; }
"#,
    )
    .unwrap();
    let generated = crust::source::SourceSet::load(&entry)
        .unwrap()
        .compile()
        .unwrap();
    let output = common::tracked_generated_c(&generated, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n"
    );
}
