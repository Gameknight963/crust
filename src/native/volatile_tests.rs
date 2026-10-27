use super::*;

#[test]
fn optimized_borrowed_and_projected_accesses_keep_volatile_loads_and_stores() {
    let source = r#"
        struct Pair { int values[2]; };
        typedef char Text[4];
        int sample(const volatile int^ value) { return *value + *value; }
        extern int read_twice(const volatile int* source) { return sample(^const *source); }
        extern void bump(volatile int* target, int increment) {
            volatile int^ value = ^*target;
            *value += increment;
        }
        extern void bump_field(volatile Pair* target, int increment) {
            target->values[0] += increment;
        }
        extern void show(volatile Text* target) { print(^const *target); }
    "#;
    let generated = crate::compile_with_kind(source, crate::ProgramKind::Library).unwrap();
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/volatile-ir")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("accesses.c");
    let ir_path = directory.join("accesses.ll");
    std::fs::write(&path, generated).unwrap();
    let mut command = Command::new("clang");
    configure_windows_toolchain(&mut command);
    let output = command
        .args(["-std=c11", "-O2", "-Werror", "-S", "-emit-llvm"])
        .arg(&path)
        .arg("-o")
        .arg(&ir_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ir = std::fs::read_to_string(ir_path).unwrap();
    assert!(
        ir.lines()
            .any(|line| line.contains("load ") && line.contains("volatile i8")),
        "{ir}"
    );
    assert_eq!(
        ir.lines()
            .filter(|line| line.contains("load ") && line.contains("volatile i32"))
            .count(),
        4,
        "{ir}"
    );
    assert_eq!(
        ir.lines()
            .filter(|line| line.contains("store ") && line.contains("volatile i32"))
            .count(),
        2,
        "{ir}"
    );
}
