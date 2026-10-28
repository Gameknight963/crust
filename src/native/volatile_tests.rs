use super::*;

#[test]
fn optimized_native_fields_preserve_volatile_accesses_and_packed_alignment() {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/native-field-ir")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&directory).unwrap();
    let header = directory.join("native.h");
    std::fs::write(&header,"#pragma pack(push,1)\nstruct Sample { char tag; volatile int value; volatile int samples[2]; };\n#pragma pack(pop)\n").unwrap();
    let source = format!(
        r#"extern header {};
        extern struct Sample {{char tag;volatile int value;volatile int samples[2];}};
        extern int read_native(const Sample* value) {{unsafe {{return value->value+value->value;}}}}
        extern void bump_native(Sample* value) {{unsafe {{value->samples[1]++;}}}}
    "#,
        serde_json::to_string(&header.to_string_lossy().replace('\\', "/")).unwrap()
    );
    let generated = crate::compile_with_kind(&source, crate::ProgramKind::Library).unwrap();
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
    let body = ir
        .split("@read_native(")
        .nth(1)
        .unwrap()
        .split("\n}")
        .next()
        .unwrap();
    assert_eq!(
        body.lines()
            .filter(|line| line.contains("load volatile i32") && line.contains("align 1"))
            .count(),
        2,
        "{ir}"
    );
    let body = ir
        .split("@bump_native(")
        .nth(1)
        .unwrap()
        .split("\n}")
        .next()
        .unwrap();
    assert!(
        body.lines()
            .any(|line| line.contains("store volatile i32") && line.contains("align 1")),
        "{ir}"
    );
}

#[test]
fn optimized_borrowed_and_projected_accesses_keep_volatile_loads_and_stores() {
    let source = r#"
        struct Pair { int values[2]; };
        typedef char Text[4];
        int sample(const volatile int& value) { return value + value; }
        extern int read_twice(const volatile int* source) { return sample(&const *source); }
        extern void bump(volatile int* target, int increment) {
            volatile int& value = &*target;
            value += increment;
        }
        extern void bump_field(volatile Pair* target, int increment) {
            target->values[0] += increment;
        }
        extern void show(volatile Text* target) { print(&const *target); }
        extern int update(volatile int* target) {
            int previous = (*target)++;
            return previous + ++*target;
        }
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
        6,
        "{ir}"
    );
    assert_eq!(
        ir.lines()
            .filter(|line| line.contains("store ") && line.contains("volatile i32"))
            .count(),
        4,
        "{ir}"
    );
}
