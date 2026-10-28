use super::*;

#[test]
fn source_location_tables_compile_without_unused_variable_warnings() {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/runtime-warning-tests")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&directory).unwrap();
    for (index, source) in [
        "int main() { return 0; }",
        "int main() { int value = 41; return value + 1; }",
        "struct View { int& value; }; int main() { int n=41; View view={.value=&n}; view.value++; return view.value; }",
    ]
    .into_iter()
    .enumerate()
    {
        let sources = crate::source::SourceSet::load_with(&directory.join("input.cr"), |_| {
            Ok(source.to_owned())
        })
        .unwrap();
        let c_path = directory.join(format!("case_{index}.c"));
        std::fs::write(&c_path, sources.compile().unwrap()).unwrap();
        let mut command = Command::new("clang");
        configure_windows_toolchain(&mut command);
        let output = command
            .args([
                "-std=c11",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-Wno-unused-function",
                "-c",
            ])
            .arg(&c_path)
            .arg("-o")
            .arg(c_path.with_extension("o"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
