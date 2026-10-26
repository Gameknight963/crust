mod common;

use crust::bindings::{Options, generate};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn header(source: &str) -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/c-bindings-tests")
        .join(std::process::id().to_string())
        .join(NEXT.fetch_add(1, Ordering::Relaxed).to_string());
    std::fs::create_dir_all(&directory).unwrap();
    let header = directory.join("native ü.h");
    std::fs::write(&header, source).unwrap();
    return header;
}

fn options(functions: &[&str]) -> Options {
    return Options {
        functions: functions.iter().map(|name| (*name).to_owned()).collect(),
        ..Options::default()
    };
}

#[test]
fn generates_and_executes_selected_opaque_handle_and_callback_bindings() {
    let path = header(
        r#"
        typedef struct Handle Handle;
        typedef int (*Callback)(int);
        Handle *create(int);
        int apply(Handle *, Callback);
        void destroy(Handle *);
    "#,
    );
    let bindings = generate(&path, &options(&["create", "apply", "destroy"])).unwrap();
    assert!(bindings.contains("struct Handle;"));
    assert!(bindings.contains("c_destroy"));
    assert!(bindings.contains("= \"destroy\""));
    let source = format!(
        r#"
        {bindings}
        int twice(int value) {{ return value * 2; }}
        int main() {{ unsafe {{ Handle* handle = create(21); print(apply(handle, twice)); c_destroy(handle); }} return 0; }}
    "#
    );
    let mut generated = crust::compile(&source).unwrap();
    generated.push_str(r#"
        struct Handle { int value; };
        struct Handle *create(int value) { struct Handle *handle = malloc(sizeof(*handle)); handle->value = value; return handle; }
        int apply(struct Handle *handle, int (*callback)(int)) { return callback(handle->value); }
        void destroy(struct Handle *handle) { free(handle); }
    "#);
    let output = common::tracked_generated_c(&generated, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        "drop:21\n"
    );
}

#[test]
fn preserves_native_integer_types_const_pointer_levels_variadics_and_callback_returns() {
    let path = header(
        r#"
        typedef unsigned long Size;
        typedef long long (*Callback)(const char *, Size);
        Callback factory(void);
        long inspect(int *const *, const int *, _Bool, int, ...);
    "#,
    );
    let bindings = generate(&path, &options(&["factory", "inspect"])).unwrap();
    assert!(bindings.contains("long long"));
    assert!(bindings.contains("unsigned long"));
    assert!(bindings.contains("int* const*"));
    assert!(bindings.contains("const int*"));
    assert!(bindings.contains("bool"));
    assert!(bindings.contains("..."));
    crust::check(&format!("{bindings} int main() {{ return 0; }}")).unwrap();
}

#[cfg(all(windows, any(target_arch = "x86", target_arch = "x86_64")))]
#[test]
fn preserves_vectorcall_in_native_and_nested_callback_signatures() {
    let path =
        header("typedef int (__vectorcall *Callback)(int); int __vectorcall apply(Callback, int);");
    let bindings = generate(&path, &options(&["apply"])).unwrap();
    assert!(bindings.matches("__vectorcall").count() >= 2);
}

#[test]
fn rejects_internal_linkage_missing_exports_and_unsupported_types() {
    for (source, name, expected) in [
        (
            "static int hidden(int n) { return n; }",
            "hidden",
            "internal linkage",
        ),
        ("int present(void);", "missing", "no function declaration"),
        ("double floating(double);", "floating", "scalar type"),
        (
            "struct Item { int value; }; struct Item value(void);",
            "value",
            "by value",
        ),
        ("int inspect(volatile int*);", "inspect", "qualifiers"),
        ("int old();", "old", "complete C function prototype"),
    ] {
        let path = header(source);
        let error = generate(&path, &options(&[name])).unwrap_err();
        assert!(error.contains(expected), "{name}: {error}");
    }
}

#[test]
fn rejects_invalid_selections_and_reports_clang_failures() {
    let path = header("int native(void);");
    for functions in [
        vec![],
        vec!["native", "native"],
        vec!["native();"],
        vec!["main"],
    ] {
        assert!(generate(&path, &options(&functions)).is_err());
    }
    let mut missing = options(&["native"]);
    for defines in [vec!["=1"], vec!["VALUE=1", "VALUE=2"]] {
        missing.defines = defines.into_iter().map(str::to_owned).collect();
        assert!(generate(&path, &missing).is_err());
    }
    missing.defines.clear();
    missing.clang = Some(PathBuf::from("missing-crust-test-clang"));
    assert!(
        generate(&path, &missing)
            .unwrap_err()
            .contains("could not start Clang")
    );
    let broken = header("int native(;");
    assert!(
        generate(&broken, &options(&["native"]))
            .unwrap_err()
            .contains("C header analysis failed")
    );
}

#[test]
fn imports_standard_library_prototypes_and_preserves_header_preprocessing() {
    let path = header("#include <stdio.h>\n#include <stdlib.h>\n");
    let bindings = generate(
        &path,
        &options(&["puts", "fopen", "fclose", "snprintf", "malloc", "free"]),
    )
    .unwrap();
    assert!(bindings.contains("extern"));
    crust::check(&format!("{bindings} int main() {{ return 0; }}")).unwrap();
    let path = header("#include <native_types.h>\nNativeSize selected(NativeSize);\n");
    let include = path.parent().unwrap().join("include");
    std::fs::create_dir_all(&include).unwrap();
    std::fs::write(include.join("native_types.h"), "#ifdef WIDE_API\ntypedef unsigned long long NativeSize;\n#else\ntypedef int NativeSize;\n#endif\n").unwrap();
    let mut selected = options(&["selected"]);
    selected.include_dirs.push(include);
    selected.defines.push("WIDE_API=1".to_owned());
    let bindings = generate(&path, &selected).unwrap();
    assert!(bindings.contains("unsigned long long"));
}

#[test]
fn generated_names_avoid_collisions_with_selected_native_names() {
    let path = header("int destroy(int); int c_destroy(int); int print(int); int move(int);");
    let bindings = generate(&path, &options(&["destroy", "c_destroy", "print", "move"])).unwrap();
    assert!(bindings.contains("c_destroy_(int) = \"destroy\""));
    assert!(bindings.contains("c_print(int) = \"print\""));
    assert!(bindings.contains("c_move(int) = \"move\""));
}

#[test]
fn filtered_declarations_keep_exact_function_matches_and_reachable_type_names() {
    let path = header(
        r#"
        int selected_extra(char*);
        int selected(int);
        struct CrustCFunction0;
        typedef int (*Callback)(int);
        int other(struct CrustCFunction0*, Callback);
    "#,
    );
    let bindings = generate(&path, &options(&["selected", "other"])).unwrap();
    assert!(bindings.contains("extern int selected(int);"));
    assert!(!bindings.contains("selected_extra"));
    assert!(bindings.contains("struct CrustCFunction0;"));
    assert!(!bindings.contains("typedef int CrustCFunction0("));
}

#[test]
fn function_macros_do_not_redirect_the_selected_native_symbol_or_its_type() {
    let path = header("int original(int); int alternate(char*);\n#define original alternate\n");
    let bindings = generate(&path, &options(&["original"])).unwrap();
    assert!(bindings.contains("extern int original(int);"));
    assert!(!bindings.contains("char*"));
}

#[test]
fn cli_writes_utf8_bindings_and_protects_existing_outputs_and_headers() {
    let path = header("int increment(int);");
    let output = path.with_file_name("bindings.cr");
    let command = || {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_crust"));
        command
            .arg("import-c")
            .arg(&path)
            .args(["--function", "increment", "--output"])
            .arg(&output);
        return command;
    };
    let imported = command().output().unwrap();
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    let text = std::fs::read_to_string(&output).unwrap();
    assert!(text.contains("extern int increment(int);"));
    assert!(!command().output().unwrap().status.success());
    assert!(command().arg("--force").output().unwrap().status.success());
    let protected = std::process::Command::new(env!("CARGO_BIN_EXE_crust"))
        .arg("import-c")
        .arg(&path)
        .args(["--function", "increment", "--output"])
        .arg(&path)
        .arg("--force")
        .output()
        .unwrap();
    assert!(!protected.status.success());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "int increment(int);"
    );
    let unknown = std::process::Command::new(env!("CARGO_BIN_EXE_crust"))
        .args(["import-c", "--unknown"])
        .output()
        .unwrap();
    assert!(!unknown.status.success());
}

#[test]
fn header_example_uses_reproducible_bindings_and_destroys_its_moved_c_resource() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/header_project");
    let mut selected = options(&[
        "native_create",
        "native_value",
        "native_add",
        "native_destroy",
    ]);
    selected.variables.push("native_limit".to_owned());
    let generated = generate(&root.join("native.h"), &selected).unwrap();
    assert_eq!(
        generated.replace("\r\n", "\n"),
        include_str!("../examples/header_project/bindings.cr").replace("\r\n", "\n")
    );
    let sources = crust::source::SourceSet::load(&root.join("main.cr")).unwrap();
    let mut c = sources.compile().unwrap();
    c.push_str(include_str!("../examples/header_project/native.h"));
    c.push_str(
        &include_str!("../examples/header_project/native.c").replace("#include \"native.h\"", ""),
    );
    let output = common::tracked_generated_c(&c, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n100\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        "drop:42\n"
    );
}

#[test]
fn generates_native_mutable_const_pointer_and_callback_variables() {
    let path = header(
        r#"
        typedef int (*Callback)(int);
        extern int counter;
        extern const int limit;
        extern int *pointer;
        extern int *const fixed;
        extern Callback callback;
    "#,
    );
    let mut options = options(&[]);
    options.variables = ["counter", "limit", "pointer", "fixed", "callback"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let bindings = generate(&path, &options).unwrap();
    assert!(bindings.contains("extern const int limit;"));
    assert!(bindings.contains("extern int* const fixed;"));
    let source = format!(
        "{bindings} int main() {{ unsafe {{ counter = 40; *fixed += 2; print(callback(*pointer)); print(limit); }} return 0; }}"
    );
    let mut generated = crust::compile(&source).unwrap();
    generated.push_str("int counter = 1; const int limit = 100; int* pointer = &counter; int* const fixed = &counter; static int increment(int n) { return n + 1; } int (*callback)(int) = increment;");
    let output = common::tracked_generated_c(&generated, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "43\n100\n"
    );
}

#[test]
fn cli_can_select_functions_and_variables_from_one_header() {
    let path = header("extern int init; int read_value(void);");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_crust"))
        .arg("import-c")
        .arg(&path)
        .args(["--variable", "init", "--function", "read_value"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bindings = String::from_utf8(output.stdout).unwrap();
    assert!(bindings.contains("extern int c_init = \"init\";"));
    assert!(bindings.contains("extern int read_value();"));
}

#[test]
fn rejects_variable_storage_requiring_native_wrappers_and_checks_all_redeclarations() {
    for source in [
        "static int value;",
        "extern _Thread_local int value;",
        "extern volatile int value;",
        "extern int value[4];",
        "struct Item { int n; }; extern struct Item value;",
        "int value(void);",
    ] {
        let path = header(source);
        let mut selected = options(&[]);
        selected.variables.push("value".to_owned());
        assert!(generate(&path, &selected).is_err(), "{source}");
    }
    let path = header("int selected(void); int selected(void) __asm__(\"actual_symbol\");");
    assert!(generate(&path, &options(&["selected"])).is_err());
}

#[cfg(windows)]
#[test]
fn dll_import_variable_attributes_on_later_declarations_require_a_native_wrapper() {
    let path = header("extern int value; __declspec(dllimport) extern int value;");
    let mut selected = options(&[]);
    selected.variables.push("value".to_owned());
    assert!(generate(&path, &selected).is_err());
}
