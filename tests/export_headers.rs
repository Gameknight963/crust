mod common;

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn directory() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/export-headers")
        .join(format!(
            "{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    fs::create_dir_all(&path).unwrap();
    return path;
}

fn run(generated: &str, header: &str, consumer: &str, expected: &str) {
    let directory = directory();
    let path = directory.join("public.h");
    fs::write(&path, header).unwrap();
    let path = path.to_string_lossy().replace('\\', "/");
    let c = format!("#include \"{path}\"\n#include \"{path}\"\n{generated}\n{consumer}");
    let output = common::tracked_generated_c(&c, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        expected
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn headers_preserve_scalar_pointer_const_callback_and_returned_callback_abis() {
    let source = r#"
        typedef int (*Operation)(int);
        typedef Operation (*Factory)();
        typedef int (*Variadic)(int, ...);
        enum Flags : unsigned short { Enabled = 3 };
        int private_helper(int value) { return value; }
        extern int imported_private(int);
        extern int apply(Operation callback, int value) { return callback(value); }
        extern Operation select(Factory factory) = "example_select" { return factory(); }
        extern const char* text(const char* const* slot) { return *slot; }
        extern bool equal(int left, int right) { return left == right; }
        extern unsigned long long wide(unsigned long long value) { return value; }
        extern Flags flags() { return Enabled; }
        extern int variadic(Variadic callback) { return callback(0, (unsigned short)65535); }
    "#;
    let kind = crust::ProgramKind::Library;
    let header = crust::emit_header(source, kind).unwrap();
    assert!(!header.contains("private_helper"));
    assert!(!header.contains("imported_private"));
    run(
        &crust::compile_with_kind(source, kind).unwrap(),
        &header,
        r#"
        #include <stdarg.h>
        typedef int32_t (*NativeOperation)(int32_t);
        int32_t plus_one(int32_t value) { return value + 1; }
        NativeOperation factory(void) { return plus_one; }
        int32_t vararg(int32_t tag, ...) { va_list args; va_start(args, tag); int value = va_arg(args, int); va_end(args); return value; }
        int main(void) {
            const char *value = "hello";
            printf("%d\n", (int)apply(plus_one, 41));
            printf("%d\n", (int)example_select(factory)(6));
            printf("%s\n", text(&value));
            printf("%d\n", (int)equal(3, 3));
            printf("%llu\n", wide(18446744073709551615ULL));
            printf("%u\n", (unsigned)flags());
            printf("%d\n", (int)variadic(vararg));
            return 0;
        }
    "#,
        "42\n7\nhello\n1\n18446744073709551615\n3\n65535\n",
    );
}

#[test]
fn headers_keep_native_enum_identity_and_opaque_tags_when_c_macros_share_their_names() {
    let directory = directory();
    fs::write(directory.join("native.h"), "#ifndef NATIVE_HEADER\n#define NATIVE_HEADER\nstruct Handle; union Storage; enum NativeMode { NativeNegative = -1, NativePositive = 2, crust_header_choose_0 = 3 };\n#define Handle 47\n#define Storage 48\n#define NativeMode 49\n#endif\n").unwrap();
    fs::write(
        directory.join("library.cr"),
        r#"
        extern header "native.h";
        struct Handle;
        union Storage;
        extern enum NativeMode : int { NativeNegative = -1, NativePositive = 2, crust_header_choose_0 = 3 };
        extern Handle* echo(Handle* value) = "native_echo" { return value; }
        extern Storage* pass(Storage* value) { return value; }
        extern NativeMode choose(NativeMode value) {
            if (value == NativePositive) { return NativeNegative; }
            return NativePositive;
        }
    "#,
    )
    .unwrap();
    let sources = crust::source::SourceSet::load(&directory.join("library.cr")).unwrap();
    let kind = crust::ProgramKind::Library;
    let header = sources.emit_header(kind).unwrap();
    run(
        &sources.compile_with_kind(kind).unwrap(),
        &format!(
            "{header}\n#if Handle != 47 || Storage != 48 || NativeMode != 49\n#error native macros must survive the public header\n#endif\n"
        ),
        r#"
        int main(void) {
            printf("%d\n", (int)choose(NativePositive));
            printf("%d\n", native_echo(0) == 0);
            printf("%d\n", pass(0) == 0);
            return 0;
        }
    "#,
        "-1\n1\n1\n",
    );
}

#[test]
#[cfg(windows)]
fn headers_preserve_vectorcall_in_exports_and_callback_parameters() {
    let source = "typedef int (__vectorcall *Callback)(int); extern int __vectorcall apply(Callback callback, int value) { return callback(value); }";
    let kind = crust::ProgramKind::Library;
    run(
        &crust::compile_with_kind(source, kind).unwrap(),
        &crust::emit_header(source, kind).unwrap(),
        "int32_t __vectorcall plus_one(int32_t value) { return value + 1; } int main(void) { printf(\"%d\\n\", (int)apply(plus_one, 41)); return 0; }",
        "42\n",
    );
}

#[test]
fn headers_report_aggregate_layout_requirements_and_check_function_bodies() {
    let kind = crust::ProgramKind::Library;
    for source in [
        "struct Value { int field; }; extern Value echo(Value value) { return value; }",
        "struct Value { int field; }; extern Value* echo(Value* value) { return value; }",
        "typedef int Values[2]; extern Values* echo(Values* value) { return value; }",
    ] {
        let error = crust::emit_header(source, kind).unwrap_err();
        assert!(error.message.contains("C header generation"), "{error}");
    }
    assert!(crust::emit_header("extern int broken(int value) { return missing; }", kind).is_err());
}
