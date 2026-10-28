use super::*;

#[test]
fn runtime_header_macros_do_not_rewrite_imported_functions() {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/runtime-header-macro-tests")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&directory).unwrap();
    // Model the fortified function macros supplied by Darwin's string.h.
    std::fs::write(directory.join("string.h"), "#include <stddef.h>\nvoid *memcpy(void *, const void *, size_t);\n#define memcpy(d,s,n) __builtin___memcpy_chk(d,s,n,__builtin_object_size(d,0))\n").unwrap();
    let generated = crate::compile(
        "extern void* memcpy(void*, const void*, size_t); int main() { int source=42; int destination=0; unsafe { memcpy((void*)&destination, (const void*)&source, sizeof(int)); } print(destination); return 0; }",
    )
    .unwrap();
    let c_path = directory.join("main.c");
    std::fs::write(&c_path, generated).unwrap();
    let executable = directory.join(format!("main{}", std::env::consts::EXE_SUFFIX));
    compile_file_with_options(
        &c_path,
        &executable,
        &Options {
            include_dirs: vec![directory],
            ..Default::default()
        },
    )
    .unwrap();
    let output = Command::new(executable).output().unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");
}

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
