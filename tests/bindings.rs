mod common;
use common::compiler as crust;

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
fn cli_imports_macro_constants_using_the_selected_header_configuration() {
    let path = header("#define FEATURE_VALUE (FEATURE + 1u)\n");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_crust"))
        .arg("import-c")
        .arg(&path)
        .args(["--macro", "FEATURE_VALUE", "--define", "FEATURE=41u"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bindings = String::from_utf8(output.stdout).unwrap();
    let output = common::tracked_run(&format!(
        "{bindings} int main() {{ print(FEATURE_VALUE); return 0; }}"
    ));
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n"
    );
    assert!(
        crust::compile(&format!(
            "{bindings} int main() {{ unsafe {{ FEATURE_VALUE=1; }} return 0; }}"
        ))
        .is_err()
    );
}

#[test]
fn imports_integer_and_boolean_macros_with_native_types_and_exact_values() {
    let path = header(
        r#"
        #include <stdint.h>
        #define COUNT (2 + 3 * 4)
        #define NEGATIVE ((short)-7)
        #define MAXIMUM UINT64_C(18446744073709551615)
        #define MINIMUM (-9223372036854775807LL - 1LL)
        #define FLAG ((_Bool)1)
        #define unsafe 42u
        #define NATIVE_LONG ((unsigned long)123)
    "#,
    );
    let options = Options {
        macros: [
            "COUNT",
            "NEGATIVE",
            "MAXIMUM",
            "MINIMUM",
            "FLAG",
            "unsafe",
            "NATIVE_LONG",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        ..Options::default()
    };
    let bindings = generate(&path, &options).unwrap();
    assert!(bindings.contains("const unsigned long NATIVE_LONG"));
    let output = common::tracked_run(&format!(
        "{bindings} int main() {{ print(COUNT); print(NEGATIVE); print(MAXIMUM); print(MINIMUM); print(FLAG); print(c_unsafe); print(NATIVE_LONG); return 0; }}"
    ));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "14\n-7\n18446744073709551615\n-9223372036854775808\n1\n42\n123\n"
    );
}

#[test]
fn macro_bindings_reserve_selected_names_and_resolve_enum_member_collisions() {
    let path = header(
        r#"
        enum Values { SAME=3 };
        #define SAME 5
        #define unsafe 7
        #define c_unsafe 9
    "#,
    );
    let options = Options {
        enums: vec!["Values".to_owned()],
        macros: ["SAME", "unsafe", "c_unsafe"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        ..Options::default()
    };
    let bindings = generate(&path, &options).unwrap();
    let output = common::tracked_run(&format!(
        "{bindings} int main() {{ print((int)Values::SAME); print(c_SAME); print(c_unsafe_); print(c_unsafe); return 0; }}"
    ));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "3\n5\n7\n9\n"
    );
}

#[test]
fn macro_imports_reject_missing_nonconstant_and_noninteger_definitions() {
    let path = header(
        "#define FLOATING 1.5\n#define TEXT \"text\"\n#define APPLY(x) ((x)+1)\nint runtime();\n#define RUNTIME runtime()\n#define VALID 1\n",
    );
    for name in [
        "MISSING", "FLOATING", "TEXT", "APPLY", "RUNTIME", "bad-name",
    ] {
        let options = Options {
            macros: vec![name.to_owned()],
            ..Options::default()
        };
        assert!(generate(&path, &options).is_err(), "accepted {name}");
    }
    let options = Options {
        macros: vec!["VALID".to_owned(), "VALID".to_owned()],
        ..Options::default()
    };
    assert!(
        generate(&path, &options)
            .unwrap_err()
            .contains("selected twice")
    );
}

#[test]
fn imports_floating_functions_and_callbacks_with_native_abi_types() {
    let path = header(
        "float native_scale(float value); double native_apply(double (*callback)(double), double value); long double native_widen(long double value);",
    );
    let bindings = generate(
        &path,
        &options(&["native_scale", "native_apply", "native_widen"]),
    )
    .unwrap();
    let source = format!(
        "{bindings} double twice(double value) {{ return value * 2; }} int main() {{ unsafe {{ print(native_scale(3.5f)); print(native_apply(&twice, 2.0)); print(native_widen(2.5L)); }} return 0; }}"
    );
    let mut generated = crust::compile(&source).unwrap();
    generated.push_str("float native_scale(float value) { return value * 2; } double native_apply(double (*callback)(double), double value) { return callback(value); } long double native_widen(long double value) { return value + 1; }");
    let output = common::tracked_generated_c(&generated, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "7\n4\n3.5\n"
    );
}

#[test]
fn keyword_named_c_tags_import_as_opaque_aliases_and_keep_owner_cleanup() {
    let path = header(
        r#"
        struct init;
        union move;
        typedef struct init NativeHandle;
        typedef union move* NativeStorage;
        struct init* make_handle(int);
        int handle_value(const struct init*);
        void release_handle(struct init*);
        union move* make_storage(int);
        int storage_value(const union move*);
        void release_storage(union move*);
    "#,
    );
    let mut selected = options(&[
        "make_handle",
        "handle_value",
        "release_handle",
        "make_storage",
        "storage_value",
        "release_storage",
    ]);
    selected.typedefs = ["NativeHandle", "NativeStorage"]
        .map(str::to_owned)
        .to_vec();
    let bindings = generate(&path, &selected).unwrap();
    assert!(bindings.contains("struct c_init = \"init\";"));
    assert!(bindings.contains("union c_move = \"move\";"));
    let source = format!(
        r#"
        {bindings}
        struct Owner {{
            private NativeHandle* pointer;
            static Self create(int value) {{ unsafe {{ Self result = {{ .pointer = make_handle(value) }}; return move(result); }} }}
            int read() const {{ unsafe {{ return handle_value(this.pointer); }} }}
            ~Owner() {{ unsafe {{ release_handle(this.pointer); }} }}
        }};
        int main() {{
            {{ Owner value = Owner::create(42); print(value.read()); }}
            unsafe {{ NativeStorage storage = make_storage(7); print(storage_value(storage)); release_storage(storage); }}
            return 0;
        }}
    "#
    );
    let generated = format!(
        r#"{}
        struct init {{ int value; }};
        union move {{ int value; unsigned char bytes[4]; }};
        struct init* make_handle(int value) {{ struct init* result = malloc(sizeof(*result)); if (!result) abort(); result->value = value; return result; }}
        int handle_value(const struct init* value) {{ return value->value; }}
        void release_handle(struct init* value) {{ free(value); }}
        union move* make_storage(int value) {{ union move* result = malloc(sizeof(*result)); if (!result) abort(); result->value = value; return result; }}
        int storage_value(const union move* value) {{ return value->value; }}
        void release_storage(union move* value) {{ free(value); }}
    "#,
        crust::compile(&source).unwrap()
    );
    let output = common::tracked_generated_c(&generated, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n7\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        "drop:42\ndrop:7\n"
    );
}

#[test]
fn opaque_tag_aliases_reject_invalid_native_names_conflicts_and_field_definitions() {
    for (declarations, expected) in [
        ("struct Handle = 7;", "quoted native struct tag"),
        ("union Storage = \"\";", "ASCII identifiers"),
        ("struct Handle = \"bad tag\";", "ASCII identifiers"),
        ("struct Handle = \"int\";", "ASCII identifiers"),
        ("struct Handle = \"é\";", "ASCII identifiers"),
        (
            "struct Handle = \"First\"; struct Handle = \"Second\";",
            "conflicting native tags",
        ),
        (
            "struct Handle = \"Native\"; union Storage = \"Native\";",
            "already declared as struct",
        ),
        (
            "extern header \"native.h\"; extern enum Mode = \"Native\" { Value = 1 }; struct Handle = \"Native\";",
            "already declared as enum",
        ),
        (
            "struct Handle = \"Native\"; struct Handle { int value; };",
            "must remain opaque",
        ),
        (
            "union Storage = \"Native\"; union Storage { int value; };",
            "must remain opaque",
        ),
    ] {
        let error =
            crust::check(&format!("{declarations} int main() {{ return 0; }}")).unwrap_err();
        assert!(error.message.contains(expected), "{declarations}: {error}");
    }
}

#[test]
fn selected_typedefs_preserve_native_scalars_pointer_qualifiers_and_callback_types() {
    let path = header(
        r#"
        struct Native;
        typedef struct Native Native;
        typedef unsigned long NativeCount;
        typedef const char* NativeText;
        typedef int* const* NativeSlot;
        typedef volatile int* NativeRegister;
        typedef int (*NativeCallback)(NativeCount);
        typedef int NativeFunction(NativeCount);
        int use_callback(NativeCallback, NativeCount);
    "#,
    );
    let mut selected = options(&["use_callback"]);
    selected.typedefs = [
        "Native",
        "NativeCount",
        "NativeText",
        "NativeSlot",
        "NativeRegister",
        "NativeCallback",
        "NativeFunction",
    ]
    .map(str::to_owned)
    .to_vec();
    let bindings = generate(&path, &selected).unwrap();
    assert!(bindings.contains("typedef unsigned long NativeCount;"));
    assert!(bindings.contains("typedef const char* NativeText;"));
    assert!(bindings.contains("typedef int* const* NativeSlot;"));
    assert!(bindings.contains("typedef volatile int* NativeRegister;"));
    let source = format!(
        r#"
        {bindings}
        int increment(NativeCount value) {{ return (int)value + 1; }}
        int main() {{
            NativeCount count = 41;
            NativeCallback callback = increment;
            NativeFunction* function = increment;
            unsafe {{
                print(use_callback(callback, count));
                print(function(40));
                Native* handle = 0;
                NativeText text = 0;
                print(handle == 0);
                print(text == 0);
                int value = 7;
                int* pointer = &value;
                NativeSlot slot = &pointer;
                **slot = 42;
                print(value);
                NativeRegister register_value = &value;
                *register_value = 9;
                print(value);
            }}
            return 0;
        }}
    "#
    );
    let generated = format!(
        "{}\n{}\nint use_callback(NativeCallback callback, NativeCount value) {{ return callback(value); }}",
        std::fs::read_to_string(&path).unwrap(),
        crust::compile(&source).unwrap()
    );
    let output = common::tracked_generated_c(&generated, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n41\n1\n1\n42\n9\n"
    );
    let invalid = format!(
        "{bindings} int main() {{ unsafe {{ int first = 1; int second = 2; int* pointer = &first; NativeSlot slot = &pointer; *slot = &second; }} return 0; }}"
    );
    assert!(crust::check(&invalid).is_err());
}

#[test]
fn typedef_only_imports_include_native_enums_and_avoid_alias_tag_name_cycles() {
    let path = header(
        "#include <stddef.h>\ntypedef enum Mode { Off = 0, On = 42 } Mode; typedef enum PointerTag { Value = 7 } *PointerTag; typedef void Empty;",
    );
    let mut selected = options(&[]);
    selected.typedefs = ["Mode", "PointerTag", "size_t", "Empty"]
        .map(str::to_owned)
        .to_vec();
    let bindings = generate(&path, &selected).unwrap();
    assert!(bindings.contains("extern enum Mode"));
    assert!(bindings.contains("typedef Mode c_Mode;"));
    assert!(bindings.contains("typedef PointerTag* c_PointerTag;"));
    assert!(bindings.contains(" c_size_t;"));
    let source = format!(
        "{bindings} int main() {{ c_Mode mode = Mode::On; c_size_t count = 7; c_PointerTag pointer = 0; Empty* empty = 0; print((int)mode); print(count); unsafe {{ print(pointer == 0); print(empty == 0); }} return 0; }}"
    );
    let output = common::tracked_run(&source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n7\n1\n1\n"
    );
}

#[test]
fn typedef_imports_reject_layout_changes_unsupported_types_and_invalid_selections() {
    for (source, name, expected) in [
        (
            "typedef int Aligned __attribute__((aligned(32)));",
            "Aligned",
            "unsupported C typedef layout",
        ),
        ("typedef int Values[4];", "Values", "unsupported C type"),
        (
            "typedef double _Complex Real;",
            "Real",
            "unsupported C type",
        ),
        (
            "typedef const int Readonly;",
            "Readonly",
            "top-level typedef qualifiers",
        ),
        (
            "typedef int* const Slot;",
            "Slot",
            "top-level typedef qualifiers",
        ),
        ("int function(void);", "function", "no typedef declaration"),
        ("typedef int Present;", "Missing", "no typedef declaration"),
    ] {
        let path = header(source);
        let mut selected = options(&[]);
        selected.typedefs.push(name.to_owned());
        let error = generate(&path, &selected).unwrap_err();
        assert!(error.contains(expected), "{source}: {error}");
    }
    let path = header("typedef int Count;");
    let mut selected = options(&[]);
    selected.typedefs = vec!["Count".to_owned(), "Count".to_owned()];
    assert!(
        generate(&path, &selected)
            .unwrap_err()
            .contains("selected twice")
    );
}

#[test]
fn cli_imports_typedefs_with_relative_enum_headers_and_late_c_macros() {
    let path = header(
        "typedef enum Mode { Active = 42 } Mode; typedef int init; typedef long other_alias;\n#define init other_alias\n",
    );
    let binding = path.parent().unwrap().join("types.cr");
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_crust"))
        .arg("import-c")
        .arg(&path)
        .args(["--typedef", "Mode", "--typedef", "init", "--output"])
        .arg(&binding)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = std::fs::read_to_string(&binding).unwrap();
    assert!(text.contains("extern header \"native ü.h\";"));
    assert!(text.contains("typedef int c_init;"));
    assert!(!text.starts_with('\u{feff}'));
    let source = path.parent().unwrap().join("main.cr");
    std::fs::write(&source, "import \"types.cr\"; int main() { c_Mode mode = Active; c_init value = 7; print((int)mode); print(value); return 0; }").unwrap();
    let sources = crust::source::SourceSet::load(&source).unwrap();
    let output = common::tracked_generated_c(&sources.compile().unwrap(), false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n7\n"
    );
    let missing = std::process::Command::new(env!("CARGO_BIN_EXE_crust"))
        .arg("import-c")
        .arg(&path)
        .arg("--typedef")
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("--typedef needs a name"));
}

#[test]
fn generated_volatile_globals_pointer_targets_and_borrows_match_native_c_types() {
    let path = header(
        r#"
        extern volatile int counter;
        extern const volatile int limit;
        extern int* volatile slot;
        typedef int* const ReadonlyPtr;
        extern volatile ReadonlyPtr saved;
        int sample(const volatile int*);
        void update(volatile int*, int);
        int read_saved(volatile ReadonlyPtr);
        int inspect_saved(volatile ReadonlyPtr*);
        "#,
    );
    let mut selected = options(&["sample", "update", "read_saved", "inspect_saved"]);
    selected.variables = ["counter", "limit", "slot", "saved"]
        .map(str::to_owned)
        .to_vec();
    let bindings = generate(&path, &selected).unwrap();
    let source = format!(
        r#"
        {bindings}
        int read(const volatile int& value) {{ return value; }}
        int main() {{
            unsafe {{
                update(&counter, 42);
                print(sample(&counter));
                print(read(&const counter));
                print(limit);
                int* pointer = slot;
                *pointer = 7;
                print(*slot);
                print(read_saved(saved));
                print(inspect_saved(&saved));
            }}
            return 0;
        }}
    "#
    );
    let native = r#"
        typedef int* const ReadonlyPtr;
        volatile int counter = 1;
        const volatile int limit = 99;
        static int plain = 0;
        int* volatile slot = &plain;
        int* const volatile saved = &plain;
        int sample(const volatile int* value) { return *value; }
        void update(volatile int* value, int next) { *value = next; }
        int read_saved(volatile ReadonlyPtr pointer) { return *pointer; }
        int inspect_saved(volatile ReadonlyPtr* pointer) { return **pointer; }
    "#;
    let generated = format!("{}\n{native}", crust::compile(&source).unwrap());
    let output = common::tracked_generated_c(&generated, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n42\n99\n7\n7\n7\n"
    );
    assert!(output.stderr.is_empty());
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
        ("static int hidden(int n);", "hidden", "internal linkage"),
        ("int present(void);", "missing", "no function declaration"),
        (
            "double _Complex floating(double _Complex);",
            "floating",
            "unsupported C type",
        ),
        (
            "struct Item { int value; }; struct Item value(void);",
            "value",
            "by value",
        ),
        (
            "int inspect(_Atomic(int)*);",
            "inspect",
            "unsupported C type",
        ),
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
        "native_status",
        "native_add",
        "native_destroy",
    ]);
    selected.variables.push("native_limit".to_owned());
    selected.binding_dir = Some(root.clone());
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
        "42\n100\n1\n"
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
        "extern _Thread_local int value;",
        "extern _Atomic(int) value;",
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
fn dll_import_variable_attributes_on_later_declarations_are_preserved() {
    let path = header("extern int value; __declspec(dllimport) extern int value;");
    let mut selected = options(&[]);
    selected.variables.push("value".to_owned());
    assert!(
        generate(&path, &selected)
            .unwrap()
            .contains("extern __declspec(dllimport) int value;")
    );
}

#[test]
fn imports_enum_representations_values_callbacks_and_const_objects() {
    let native = r#"
        enum Status { Ready = 1 << 4, Failed = -3, Again = Ready + 1 };
        typedef enum Status (*Callback)(enum Status);
        enum Status status_echo(enum Status);
        enum Status status_apply(Callback, enum Status);
        extern const enum Status native_status;
    "#;
    let path = header(native);
    let mut selected = options(&["status_echo", "status_apply"]);
    selected.variables.push("native_status".to_owned());
    let bindings = generate(&path, &selected).unwrap();
    assert!(bindings.contains("Ready = 16"));
    assert!(bindings.contains("Failed = -3"));
    assert!(bindings.contains("Again = 17"));
    let source = format!(
        r#"
        {bindings}
        Status identity(Status value) {{ return value; }}
        int main() {{
            unsafe {{
                print(status_echo(Status::Ready) == Status::Ready);
                print(status_apply(identity, Status::Again) == Status::Again);
                print(native_status == Status::Failed);
            }}
            return 0;
        }}
    "#
    );
    let mut generated = crust::compile(&source).unwrap();
    generated.push_str("enum Status status_echo(enum Status value) { return value; } enum Status status_apply(Callback callback, enum Status value) { return callback(value); } const enum Status native_status = Failed;");
    let output = common::tracked_generated_c(&generated, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "1\n1\n1\n"
    );
}

#[test]
fn enum_probes_preserve_native_conversion_of_large_unsigned_enumerators() {
    let native = "enum Flags { High = 0xffffffff }; enum Flags flag_echo(enum Flags); long long flag_value(void); unsigned long long flag_size(void);";
    let path = header(native);
    let bindings = generate(&path, &options(&["flag_echo", "flag_value", "flag_size"])).unwrap();
    let source = format!(
        "{bindings} int main() {{ unsafe {{ print((int64_t)Flags::High == flag_value()); print(flag_echo(Flags::High) == Flags::High); print(sizeof(Flags) == flag_size()); }} return 0; }}"
    );
    let mut generated = crust::compile(&source).unwrap();
    generated.push_str("enum Flags flag_echo(enum Flags value) { return value; } long long flag_value(void) { return (long long)(enum Flags)High; } unsigned long long flag_size(void) { return sizeof(enum Flags); }");
    let output = common::tracked_generated_c(&generated, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "1\n1\n1\n"
    );
}

#[test]
fn enum_member_renaming_reserves_native_names_before_allocating_escaped_names() {
    let path = header("enum E { init = 1, c_init = 2 }; enum E echo(enum E);");
    let bindings = generate(&path, &options(&["echo"])).unwrap();
    assert!(bindings.contains("c_init_ = 1"));
    assert!(bindings.contains("c_init = 2"));
}

#[test]
fn native_enum_identity_preserves_const_pointer_parameters() {
    let path = header(
        "enum Status { Ready = 16, Failed = -3 }; int status_read(const enum Status*); void status_set(enum Status*, enum Status);",
    );
    let bindings = generate(&path, &options(&["status_read", "status_set"])).unwrap();
    let source = format!(
        "{bindings} int main() {{ Status value = Status::Ready; unsafe {{ print(status_read(&value)); status_set(&value, Status::Failed); }} print((int)value); return 0; }}"
    );
    let mut generated = crust::compile(&source).unwrap();
    generated.push_str("int status_read(const enum Status* value) { return (int)*value; } void status_set(enum Status* target, enum Status value) { *target = value; }");
    let output = common::tracked_generated_c(&generated, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "16\n-3\n"
    );
}

#[test]
fn native_enum_tags_reserved_by_crust_are_renamed_without_changing_c_identity() {
    let path = header("enum init { One = 1, Two = 2 }; enum init echo(enum init);");
    let bindings = generate(&path, &options(&["echo"])).unwrap();
    assert!(bindings.contains("extern enum c_init = \"init\""));
    let source = format!(
        "{bindings} int main() {{ unsafe {{ print(echo(c_init::Two) == c_init::Two); }} return 0; }}"
    );
    let mut generated = crust::compile(&source).unwrap();
    generated.push_str("enum init echo(enum init value) { return value; }");
    let output = common::tracked_generated_c(&generated, false);
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "1");
}

#[test]
fn enum_probes_cover_narrow_and_full_width_native_integer_representations() {
    let path = header(
        r#"
        enum __attribute__((packed)) Byte { ByteZero = 0, ByteMax = 255 };
        enum Wide : unsigned long long { WideMax = 18446744073709551615ULL };
        enum Byte byte_echo(enum Byte);
        enum Wide wide_echo(enum Wide);
    "#,
    );
    let bindings = generate(&path, &options(&["byte_echo", "wide_echo"])).unwrap();
    let source = format!(
        "{bindings} int main() {{ unsafe {{ print((int)byte_echo(Byte::ByteMax)); print((uint64_t)wide_echo(Wide::WideMax)); }} return 0; }}"
    );
    let mut generated = crust::compile(&source).unwrap();
    generated.push_str("enum Byte byte_echo(enum Byte value) { return value; } enum Wide wide_echo(enum Wide value) { return value; }");
    let output = common::tracked_generated_c(&generated, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "255\n18446744073709551615\n"
    );
}

#[test]
fn native_enum_tags_can_share_spelling_with_c_macros_and_crust_builtin_types() {
    let path = header(
        "enum bool { Yes = 1 }; enum size_t { Count = 2 }; enum bool boolean_echo(enum bool); enum size_t count_echo(enum size_t);\n#define bool unrelated_name\n",
    );
    let bindings = generate(&path, &options(&["boolean_echo", "count_echo"])).unwrap();
    assert!(bindings.contains("extern enum c_bool = \"bool\""));
    assert!(bindings.contains("extern enum c_size_t = \"size_t\""));
}

#[test]
fn cli_emits_native_header_paths_relative_to_the_binding_file() {
    let path = header("enum Status { Ready = 1, Failed = -1 }; enum Status echo(enum Status);");
    let directory = path.parent().unwrap().join("generated");
    std::fs::create_dir_all(&directory).unwrap();
    let binding = directory.join("bindings.cr");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_crust"))
        .arg("import-c")
        .arg(&path)
        .args(["--function", "echo", "--output"])
        .arg(&binding)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(&binding).unwrap();
    assert!(text.contains("extern header \"../native ü.h\";"), "{text}");
    std::fs::write(
        directory.join("main.cr"),
        "import \"bindings.cr\"; int main() { return 0; }",
    )
    .unwrap();
    crust::source::SourceSet::load(&directory.join("main.cr"))
        .unwrap()
        .check()
        .unwrap();
}

#[test]
fn unsupported_enum_shapes_fail_with_diagnostics() {
    assert!(
        crust::check("enum E = \"NativeE\" : int { A = 1 }; int main() { return 0; }")
            .unwrap_err()
            .message
            .contains("requires extern")
    );
    for (source, expected) in [
        (
            "typedef enum { A = 1 } Anonymous; Anonymous echo(Anonymous);",
            "anonymous C enums",
        ),
        (
            "enum café { Ready = 1 }; enum café echo(enum café);",
            "ASCII identifiers",
        ),
        (
            "enum E { café = 1 }; enum E echo(enum E);",
            "ASCII identifiers",
        ),
        (
            "enum __attribute__((aligned(32))) E { A = 1, B = -1 }; enum E echo(enum E);",
            "native enum layout",
        ),
    ] {
        let path = header(source);
        let error = generate(&path, &options(&["echo"])).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
    assert!(
        crust::check("extern enum E : int { A = 1 }; int main() { return 0; }")
            .unwrap_err()
            .message
            .contains("extern header")
    );
}

#[test]
fn explicit_enum_selection_imports_constant_headers_without_function_or_variable_exports() {
    let path = header(
        "enum Mode { Idle = 0, Active = 42 }; enum Unselected { Other = 3 };\n#define Mode unrelated\n",
    );
    let selected = Options {
        enums: vec!["Mode".to_owned()],
        ..Options::default()
    };
    let bindings = generate(&path, &selected).unwrap();
    assert!(bindings.contains("extern enum Mode"));
    assert!(!bindings.contains("Unselected"));
    let c = crust::compile(&format!(
        "{bindings} int main() {{ print((int)Mode::Active); return 0; }}"
    ))
    .unwrap();
    let output = common::tracked_generated_c(&c, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");
}

#[test]
fn explicit_enum_selection_deduplicates_reachable_enums_and_reserves_native_tag_names() {
    let path = header(
        "enum init { First = 1 }; enum c_init { Second = 2 }; enum __crust_import_fn_0 { Third = 3 }; enum init echo(enum init); int callback(int (*)(int));",
    );
    let mut selected = options(&["echo", "callback"]);
    selected.enums = vec![
        "init".to_owned(),
        "c_init".to_owned(),
        "__crust_import_fn_0".to_owned(),
    ];
    let bindings = generate(&path, &selected).unwrap();
    assert_eq!(bindings.matches("= \"init\"").count(), 1);
    assert!(bindings.contains("extern enum c_init_ = \"init\""));
    assert!(bindings.contains("extern enum c_init :"));
    assert!(bindings.contains("extern enum __crust_import_fn_0 :"));
    crust::check(&format!("{bindings} int main() {{ return 0; }}")).unwrap();
}

#[test]
fn explicit_enum_selection_checks_tag_names_and_requires_complete_global_definitions() {
    let path = header("enum Present { First = 1 }; enum Forward;");
    for (enums, expected) in [
        (vec!["Present", "Present"], "selected twice"),
        (vec!["Present;"], "invalid C enum selection"),
        (vec!["Missing"], "complete global definition"),
        (vec!["Forward"], "complete global definition"),
    ] {
        let selected = Options {
            enums: enums.into_iter().map(str::to_owned).collect(),
            ..Options::default()
        };
        let error = generate(&path, &selected).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_crust"))
        .arg("import-c")
        .arg(&path)
        .arg("--enum")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--enum needs a tag name"));
}

#[test]
fn cli_selects_named_enums_and_writes_relative_native_header_references() {
    let path = header("enum Status { Ready = 1, Failed = -1 }; enum Kind { Small, Large };");
    let output_path = path.parent().unwrap().join("constants.cr");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_crust"))
        .arg("import-c")
        .arg(&path)
        .args(["--enum", "Status", "--enum", "Kind", "--output"])
        .arg(&output_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(&output_path).unwrap();
    assert!(text.contains("extern header \"native ü.h\";"));
    assert!(text.contains("extern enum Status"));
    assert!(text.contains("extern enum Kind"));
    crust::check(&format!("{text} int main() {{ return 0; }}")).unwrap();
}

#[test]
fn adjusted_array_parameters_preserve_pointee_const_and_accept_restrict_callback_contracts() {
    let path = header(
        "typedef void CopyFour(int destination[restrict static 4], const int source[static const 4]); int inspect(int destination[restrict static 4], const int source[static const 4], CopyFour* copy);",
    );
    let bindings = generate(&path, &options(&["inspect"])).unwrap();
    assert!(bindings.contains("const int*"));
    let source = format!(
        r#"
        {bindings}
        unsafe void copy_four(int* destination, const int* source) {{
            for (int i = 0; i < 4; i += 1) {{ destination[i] = source[i]; }}
        }}
        int main() {{
            int destination[4] = {{ 0, 0, 0, 0 }};
            int source[4] = {{ 1, 2, 3, 42 }};
            unsafe {{ print(inspect(&destination[0], &source[0], copy_four)); }}
            print(destination[0]);
            return 0;
        }}
    "#
    );
    let mut generated = crust::compile(&source).unwrap();
    generated.push_str("int inspect(int destination[restrict static 4], const int source[static const 4], CopyFour* copy) { copy(destination, source); return destination[3]; }");
    let generated = format!("{}\n{generated}", std::fs::read_to_string(&path).unwrap());
    let output = common::tracked_generated_c(&generated, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n1\n"
    );
}

#[test]
fn imported_standard_algorithms_sort_search_and_copy_crust_arrays_with_native_callbacks() {
    let path = header("#include <stdlib.h>\n#include <string.h>\n");
    let bindings = generate(&path, &options(&["qsort", "bsearch", "memcpy"])).unwrap();
    let source = format!(
        r#"
        {bindings}
        unsafe int compare(const void* left, const void* right) {{
            int first = *(const int*)left;
            int second = *(const int*)right;
            return (first > second) - (first < second);
        }}
        int main() {{
            int values[6] = {{ 9, 1, 7, 3, 5, 2 }};
            int copy[6] = {{ 0, 0, 0, 0, 0, 0 }};
            int key = 7;
            unsafe {{
                qsort(&values[0], 6, sizeof(int), compare);
                memcpy(&copy[0], &values[0], sizeof(values));
                void* found = bsearch(&key, &copy[0], 6, sizeof(int), compare);
                print(found != 0); print(*(int*)found);
            }}
            for (int i = 0; i < 6; i += 1) {{ print(copy[i]); }}
            return 0;
        }}
    "#
    );
    let output = common::tracked_run(&source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "1\n7\n1\n2\n3\n5\n7\n9\n"
    );
}

#[test]
fn native_sorting_preserves_vector_owners_and_destroys_each_resource_once() {
    let path = header("#include <stdlib.h>\n");
    let bindings = generate(&path, &options(&["qsort"])).unwrap();
    let vector = include_str!("../lib/vector.cr")
        .replace("import \"memory.cr\";", include_str!("../lib/memory.cr"))
        .replace("import \"allocation.cr\";", &common::allocation_source());
    let source = format!(
        r#"
        {bindings}
        {vector}
        struct Resource {{ Box<int> value; ~Resource() {{ print(this.value.get()); }} }};
        unsafe int compare(const void* left, const void* right) {{
            int first = ((const Resource*)left)->value.get();
            int second = ((const Resource*)right)->value.get();
            return (first > second) - (first < second);
        }}
        int main() {{
            Vector<Resource> values = Vector<Resource>::create(); values.reserve(3);
            values.push((Resource){{ .value = Box<int>::create(3) }});
            values.push((Resource){{ .value = Box<int>::create(1) }});
            values.push((Resource){{ .value = Box<int>::create(2) }});
            size_t count = values.count();
            unsafe {{ qsort(&values.at_mut(0), count, sizeof(Resource), compare); }}
            for (size_t i = 0; i < count; i += 1) {{ print(values.at(i).value.get()); }}
            return 0;
        }}
    "#
    );
    let output = common::tracked_run_sizes(&source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "1\n2\n3\n3\n2\n1\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        format!(
            "drop:bytes:4\ndrop:bytes:4\ndrop:bytes:4\ndrop:bytes:{}\n",
            std::mem::size_of::<usize>() * 3
        )
    );
    for conflicting in [
        source.replace(
            "size_t count = values.count();",
            "size_t count = values.count(); const Resource& view = values.at(0);",
        ),
        source.replace(
            "qsort(&values.at_mut(0), count,",
            "qsort(&values.at_mut(0), values.count(),",
        ),
    ] {
        let error = crust::check(&conflicting).unwrap_err();
        assert!(error.message.contains("borrow"), "{}", error.message);
    }
}

#[test]
fn imports_static_inline_definitions_and_callbacks_without_native_bridge() {
    // No include guard: verification must include this header exactly once.
    let path = header(
        r#"
        static int native_add(int value);
        static inline int native_add(int value) { return value + 1; }
        static inline int unsafe(int value) { return native_add(value); }
        static inline int native_apply(int (*callback)(int), int value) {
            return callback(value);
        }
    "#,
    );
    let bindings = generate(&path, &options(&["native_add", "unsafe", "native_apply"])).unwrap();
    assert!(bindings.contains("extern header"));
    assert!(bindings.contains("c_unsafe"));
    let output = common::tracked_run(&format!(
        "{bindings} int twice(int value) {{ return value * 2; }} int main() {{ unsafe {{ print(native_add(40)); print(c_unsafe(41)); print(native_apply(&twice, 21)); }} return 0; }}"
    ));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "41\n42\n42\n"
    );
}

#[test]
fn static_inline_import_requires_a_header_definition() {
    let path = header("static inline int missing(int value);");
    let error = generate(&path, &options(&["missing"])).unwrap_err();
    assert!(error.contains("internal linkage"), "{error}");
}

#[test]
fn inline_opaque_api_preserves_packing_bitfields_alignment_and_owner_cleanup() {
    let path = header(
        r#"
        #include <stddef.h>
        #include <stdint.h>
        #include <stdlib.h>
        #pragma pack(push, 1)
        struct PackedSample { uint8_t tag; uint32_t value; unsigned char flag : 3; };
        #pragma pack(pop)
        struct AlignedSample { _Alignas(32) unsigned char data[32]; };
        static inline struct PackedSample *sample_create(uint32_t value) {
            struct PackedSample *result = malloc(sizeof(*result));
            if (!result) abort();
            result->tag = 7; result->value = value; result->flag = 5;
            return result;
        }
        static inline uint32_t sample_value(const struct PackedSample *sample) {
            return sample->value + sample->tag + sample->flag;
        }
        static inline size_t sample_offset(void) { return offsetof(struct PackedSample, value); }
        static inline size_t sample_alignment(void) { return _Alignof(struct AlignedSample); }
        static inline void sample_destroy(struct PackedSample *sample) { free(sample); }
    "#,
    );
    let bindings = generate(
        &path,
        &options(&[
            "sample_create",
            "sample_value",
            "sample_offset",
            "sample_alignment",
            "sample_destroy",
        ]),
    )
    .unwrap();
    let output = common::tracked_run(&format!(
        r#"
        {bindings}
        struct Sample {{
            private PackedSample* handle;
            static Self create() {{ unsafe {{ return (Self){{.handle=sample_create(30)}}; }} }}
            uint32_t value() const {{ unsafe {{ return sample_value(this.handle); }} }}
            ~Sample() {{ unsafe {{ sample_destroy(this.handle); }} }}
        }};
        int main() {{
            Sample first=Sample::create(); Sample second=move(first);
            print(second.value());
            unsafe {{ print(sample_offset()); print(sample_alignment()); }}
            return 0;
        }}
    "#
    ));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n1\n32\n"
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr).lines().count(), 1);
}

#[test]
fn imports_header_local_functions_and_storage_with_shared_native_identity() {
    let path = header(
        r#"
        static int counter;
        extern int counter;
        static const int limit = 42;
        static int *const saved = &counter;
        static volatile int unsafe = 7;
        static int next(void) { return ++counter; }
        static int read_volatile(void) { return unsafe; }
    "#,
    );
    let mut selected = options(&["next", "read_volatile"]);
    selected.variables = ["counter", "limit", "saved", "unsafe"]
        .map(str::to_owned)
        .to_vec();
    let bindings = generate(&path, &selected).unwrap();
    let output = common::tracked_run(&format!(
        r#"
        {bindings}
        int main() {{
            unsafe {{
                counter = 40; print(next()); print(*saved);
                print(limit); c_unsafe = 9; print(read_volatile());
            }}
            return 0;
        }}
    "#
    ));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "41\n41\n42\n9\n"
    );
    assert!(
        crust::check(&format!(
            "{bindings} int main() {{ print(counter); return 0; }}"
        ))
        .is_err()
    );
    assert!(
        crust::check(&format!(
            "{bindings} int main() {{ unsafe {{ limit=1; }} return 0; }}"
        ))
        .is_err()
    );
}

#[test]
fn imports_zero_initialized_header_storage_without_selecting_functions() {
    let path = header("static int value;");
    let mut selected = options(&[]);
    selected.variables.push("value".to_owned());
    let bindings = generate(&path, &selected).unwrap();
    assert!(bindings.contains("extern header"));
    let output = common::tracked_run(&format!(
        "{bindings} int main() {{ unsafe {{ print(value); value=42; print(value); }} return 0; }}"
    ));
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "0\n42\n"
    );
}

#[test]
fn header_only_example_uses_relative_native_headers_and_drops_its_owner() {
    common::assert_example(
        "examples/header_only_project/main.cr",
        "42\n1\n",
        "drop:bytes:6\n",
        true,
    );
}

#[test]
fn native_header_wrappers_cover_extended_c_storage_types_arrays_and_macros() {
    let path = header(
        r#"
        #include <stdatomic.h>
        #include <stdint.h>
        #include <stdlib.h>
        #define FEATURE_TWICE(value) ((value) + (value))
        #define FEATURE_TEXT "Crust"
        #define FEATURE_SCALE 1.5L
        static _Atomic(int) feature_counter = 4;
        static _Thread_local int feature_thread_counter;
        struct NativeBytes { size_t length; unsigned char bytes[]; };
        static int feature_atomic_add(int value) {
            return atomic_fetch_add_explicit(&feature_counter, value, memory_order_relaxed);
        }
        static int feature_thread_next(void) { return ++feature_thread_counter; }
        static struct NativeBytes *feature_bytes_create(size_t length) {
            struct NativeBytes *value = malloc(sizeof(*value) + length);
            if (!value) abort();
            value->length = length;
            for (size_t i=0; i<length; ++i) value->bytes[i] = (unsigned char)(i + 1);
            return value;
        }
        static int feature_bytes_sum(const struct NativeBytes *value) {
            int result = 0;
            for (size_t i=0; i<value->length; ++i) result += value->bytes[i];
            return result;
        }
        static void feature_bytes_destroy(struct NativeBytes *value) { free(value); }
        static int feature_variable_array(int length) {
            int scratch[length];
            int result = 0;
            for (int i=0; i<length; ++i) { scratch[i] = i + 1; result += scratch[i]; }
            return result;
        }
        static double feature_complex_sum(double real, double imaginary) {
            double _Complex value;
            __real__ value = real; __imag__ value = imaginary;
            return __real__ value + __imag__ value;
        }
        static uint64_t feature_wide_carry(uint64_t left, uint64_t right) {
            unsigned __int128 value = (unsigned __int128)left + right;
            return (uint64_t)(value >> 64);
        }
        static int feature_twice(int value) { return FEATURE_TWICE(value); }
        static unsigned char feature_text_first(void) { return FEATURE_TEXT[0]; }
        static long double feature_scale(void) { return FEATURE_SCALE; }
    "#,
    );
    let bindings = generate(
        &path,
        &options(&[
            "feature_atomic_add",
            "feature_thread_next",
            "feature_bytes_create",
            "feature_bytes_sum",
            "feature_bytes_destroy",
            "feature_variable_array",
            "feature_complex_sum",
            "feature_wide_carry",
            "feature_twice",
            "feature_text_first",
            "feature_scale",
        ]),
    )
    .unwrap();
    let output = common::tracked_run(&format!(
        r#"
        {bindings}
        int main() {{
            unsafe {{
                print(feature_atomic_add(5)); print(feature_atomic_add(0));
                print(feature_thread_next()); print(feature_thread_next());
                NativeBytes* bytes=feature_bytes_create(4);
                print(feature_bytes_sum(bytes)); feature_bytes_destroy(bytes);
                print(feature_variable_array(4)); print(feature_complex_sum(2.5, 5.0));
                print(feature_wide_carry(~(uint64_t)0, 1));
                print(feature_twice(21)); print(feature_text_first()); print(feature_scale());
            }}
            return 0;
        }}
    "#
    ));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "4\n9\n1\n2\n10\n10\n7.5\n1\n42\n67\n1.5\n"
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr).lines().count(), 1);
}
