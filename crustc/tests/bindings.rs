mod common;
use common::compiler as crustc;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn header(source: &str) -> PathBuf {
    let directory = PathBuf::from(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap(),
    )
    .join("target/c-bindings-tests")
    .join(std::process::id().to_string())
    .join(NEXT.fetch_add(1, Ordering::Relaxed).to_string());
    std::fs::create_dir_all(&directory).unwrap();
    let header = directory.join("native ü.h");
    std::fs::write(&header, source).unwrap();
    return header;
}

#[test]
fn imports_floating_functions_and_callbacks_with_native_abi_types() {
    let path = header(
        "float native_scale(float value); double native_apply(double (*callback)(double), double value); long double native_widen(long double value);",
    );
    let bindings = common::native_header(&path);
    let source = format!(
        "{bindings} double twice(double value) {{ return value * 2; }} int main() {{ unsafe {{ print(native_scale(3.5f)); print(native_apply(&twice, 2.0)); print(native_widen(2.5L)); }} return 0; }}"
    );
    let output = common::tracked_with_native(
        &source,
        "float native_scale(float value) { return value * 2; } double native_apply(double (*callback)(double), double value) { return callback(value); } long double native_widen(long double value) { return value + 1; }",
    );
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

    let bindings = common::native_header(&path);

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
    let native = r#"
        struct init { int value; };
        union move { int value; unsigned char bytes[4]; };
        struct init* make_handle(int value) { struct init* result = malloc(sizeof(*result)); if (!result) abort(); result->value = value; return result; }
        int handle_value(const struct init* value) { return value->value; }
        void release_handle(struct init* value) { free(value); }
        union move* make_storage(int value) { union move* result = malloc(sizeof(*result)); if (!result) abort(); result->value = value; return result; }
        int storage_value(const union move* value) { return value->value; }
        void release_storage(union move* value) { free(value); }
    "#;
    let output = common::tracked_with_native(&source, native);
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
            crustc::check(&format!("{declarations} int main() {{ return 0; }}")).unwrap_err();
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

    let bindings = common::native_header(&path);

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
    let output = common::tracked_with_native(
        &source,
        "int use_callback(NativeCallback callback, NativeCount value) {return callback(value);}",
    );
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
    assert!(crustc::check(&invalid).is_err());
}

#[test]
fn typedef_only_imports_include_native_enums_and_avoid_alias_tag_name_cycles() {
    let path = header(
        "#include <stddef.h>\ntypedef enum Mode { Off = 0, On = 42 } Mode; typedef enum PointerTag { Value = 7 } *PointerTag; typedef void Empty;",
    );

    let bindings = common::native_header(&path);

    let source = format!(
        "{bindings} int main() {{ c_Mode mode = Mode::On; size_t count = 7; c_PointerTag pointer = 0; Empty* empty = 0; print((int)mode); print(count); unsafe {{ print(pointer == 0); print(empty == 0); }} return 0; }}"
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
fn fixed_array_typedefs_preserve_dimensions_elements_and_callback_declarators() {
    let path = header(
        r#"
        #include <stddef.h>
        typedef int Row[3];
        typedef Row Matrix[2];
        typedef const int *Pointers[2];
        typedef int (*Callbacks[2])(int);
        struct Item { int value; };
        typedef struct Item Items[2];
        static int sum(const int values[static 3]) { return values[0]+values[1]+values[2]; }
        static int apply(int (*const callback)(int), int value) { return callback(value); }
    "#,
    );

    let bindings = common::native_header(&path);

    let output = common::tracked_run(&format!(
        r#"
        {bindings}
        int next(int value) {{ return value+1; }}
        int main() {{
            Row row={{10,20,12}};
            Matrix matrix={{{{1,2,3}},{{4,5,6}}}};
            int value=42;
            Pointers pointers={{&value, &value}};
            Callbacks callbacks={{next, next}};
            Items items={{{{.value=40}}, {{.value=2}}}};
            unsafe {{
                print(sum(&row[0])); print(matrix[1][2]);
                print(*pointers[0]); print(apply(callbacks[1],41));
            }}
            print(items[0].value+items[1].value);
            print(sizeof(Matrix)==sizeof(int)*6);
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
        "42\n6\n42\n42\n42\n1\n"
    );
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

    let bindings = common::native_header(&path);
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
    let output = common::tracked_with_native(&source, native);
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
    let bindings = common::native_header(&path);

    let source = format!(
        r#"
        {bindings}
        int twice(int value) {{ return value * 2; }}
        int main() {{ unsafe {{ Handle* handle = create(21); print(apply(handle, twice)); c_destroy(handle); }} return 0; }}
    "#
    );
    let output = common::tracked_with_native(
        &source,
        r#"
        struct Handle { int value; };
        struct Handle *create(int value) { struct Handle *handle = malloc(sizeof(*handle)); handle->value = value; return handle; }
        int apply(struct Handle *handle, int (*callback)(int)) { return callback(handle->value); }
        void destroy(struct Handle *handle) { free(handle); }
    "#,
    );
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
    let bindings = common::native_header(&path);

    crustc::check(&format!("{bindings} int main() {{ Callback callback=factory(); int value=1; int* pointer=&value; unsafe{{inspect(&pointer,&value,true,1);callback(0,0);}} return 0; }}")).unwrap();
}

#[cfg(all(windows, any(target_arch = "x86", target_arch = "x86_64")))]
#[test]
fn preserves_vectorcall_in_native_and_nested_callback_signatures() {
    let path = header(
        "typedef int (__vectorcall *Callback)(int); static int __vectorcall apply(Callback callback,int value){return callback(value);}",
    );
    let source = format!(
        "{} int __vectorcall twice(int value){{return value*2;}} int main(){{print(apply(&twice,21));return 0;}}",
        common::native_header(&path)
    );
    let output = common::tracked_run(&source);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");
}

#[test]
fn imports_standard_library_prototypes_and_preserves_header_preprocessing() {
    let path = header("#include <stdio.h>\n#include <stdlib.h>\n");
    crustc::check(&format!("{} int main(){{unsafe{{void* pointer=malloc(4);free(pointer);}}char text[1]={{0}};unsafe{{return puts(&text[0]);}}}}",common::native_header(&path))).unwrap();
    let path = header(
        "#include <native_types.h>\nstatic NativeSize selected(NativeSize value){return value;}\n",
    );
    let include = path.parent().unwrap().join("include");
    std::fs::create_dir_all(&include).unwrap();
    std::fs::write(include.join("native_types.h"),"#ifdef WIDE_API\ntypedef unsigned long long NativeSize;\n#else\ntypedef int NativeSize;\n#endif\n").unwrap();
    let entry = path.with_extension("cr");
    std::fs::write(
        &entry,
        format!(
            "{} int main(){{print(selected(4294967296ULL));return 0;}}",
            common::native_header(&path)
        ),
    )
    .unwrap();
    let options = ::crustc::native::Options {
        include_dirs: vec![include],
        defines: vec!["WIDE_API=1".to_owned()],
        ..Default::default()
    };
    let sources = ::crustc::source::SourceSet::load_with_options(&entry, &options, |path| {
        std::fs::read_to_string(path)
    })
    .unwrap();
    let output = common::tracked_generated_c(&sources.compile().unwrap(), false);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "4294967296");
}

#[test]
fn native_names_avoid_collisions_with_crust_builtins() {
    let path = header(
        "static int destroy(int n){return n;} static int c_destroy(int n){return n+1;} static int print(int n){return n+2;} static int move(int n){return n+3;}",
    );
    let source = format!(
        "{} int main(){{print(c_destroy_(39));print(c_destroy(39));print(c_print(39));print(c_move(39));return 0;}}",
        common::native_header(&path)
    );
    let output = common::tracked_run(&source);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "39\n40\n41\n42\n"
    );
}

#[test]
fn function_macros_redirect_calls_before_native_type_discovery() {
    let path = header(
        "static int original(int n){return n;} static int alternate(char* text){return text[0];}\n#define original alternate\n",
    );
    let source = format!(
        "{} int main(){{char text[2]=\"*\";unsafe{{print(original(&text[0]));}}return 0;}}",
        common::native_header(&path)
    );
    let output = common::tracked_run(&source);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");
}

#[test]
fn header_example_includes_its_api_and_destroys_its_moved_c_resource() {
    let root = PathBuf::from(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap(),
    )
    .join("examples/header_project");

    let output = common::tracked_file(
        &root.join("main.cr"),
        include_str!("../../examples/header_project/native.c"),
        false,
    );
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

    let bindings = common::native_header(&path);

    let source = format!(
        "{bindings} int main() {{ unsafe {{ counter = 40; *fixed += 2; print(callback(*pointer)); print(limit); }} return 0; }}"
    );
    let output = common::tracked_with_native(
        &source,
        "int counter = 1; const int limit = 100; int* pointer = &counter; int* const fixed = &counter; static int increment(int n) { return n + 1; } int (*callback)(int) = increment;",
    );
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
fn assembler_labels_retain_native_headers_and_link_to_the_declared_symbol() {
    let path = header("int selected(void); int selected(void) __asm__(\"actual_symbol\");");
    let bindings = common::native_header(&path);

    let generated = crustc::compile(&format!(
        "{bindings} int main() {{ unsafe {{ print(selected()); }} return 0; }}"
    ))
    .unwrap();
    let output = common::tracked_generated_c(
        &format!(
            "{generated}\nint implementation(void) __asm__(\"actual_symbol\");\nint implementation(void) {{ return 42; }}"
        ),
        false,
    );
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n"
    );
}

#[cfg(windows)]
#[test]
fn dll_import_variable_attributes_on_later_declarations_are_preserved() {
    let path = header("extern int value; __declspec(dllimport) extern int value;");
    let sources = format!(
        "{} int main(){{unsafe{{print(value);}}return 0;}}",
        common::native_header(&path)
    );
    let generated = crustc::compile(&sources).unwrap();
    assert!(generated.contains("dllimport"));
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

    let bindings = common::native_header(&path);

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
    let output = common::tracked_with_native(
        &source,
        "enum Status status_echo(enum Status value) { return value; } enum Status status_apply(Callback callback, enum Status value) { return callback(value); } const enum Status native_status = Failed;",
    );
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
    let bindings = common::native_header(&path);
    let source = format!(
        "{bindings} int main() {{ unsafe {{ print((int64_t)Flags::High == flag_value()); print(flag_echo(Flags::High) == Flags::High); print(sizeof(Flags) == flag_size()); }} return 0; }}"
    );
    let output = common::tracked_with_native(
        &source,
        "enum Flags flag_echo(enum Flags value) { return value; } long long flag_value(void) { return (long long)(enum Flags)High; } unsigned long long flag_size(void) { return sizeof(enum Flags); }",
    );
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
fn enum_member_names_avoid_collisions_with_escaped_names() {
    let path = header("enum E { init=1,c_init=2 }; static enum E echo(enum E n){return n;}");
    let source = format!(
        "{} int main(){{print((int)echo(E::c_init_));print((int)echo(E::c_init));return 0;}}",
        common::native_header(&path)
    );
    let output = common::tracked_run(&source);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "1\n2\n"
    );
}

#[test]
fn native_enum_identity_preserves_const_pointer_parameters() {
    let path = header(
        "enum Status { Ready = 16, Failed = -3 }; int status_read(const enum Status*); void status_set(enum Status*, enum Status);",
    );
    let bindings = common::native_header(&path);
    let source = format!(
        "{bindings} int main() {{ Status value = Status::Ready; unsafe {{ print(status_read(&value)); status_set(&value, Status::Failed); }} print((int)value); return 0; }}"
    );
    let output = common::tracked_with_native(
        &source,
        "int status_read(const enum Status* value) { return (int)*value; } void status_set(enum Status* target, enum Status value) { *target = value; }",
    );
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
    let bindings = common::native_header(&path);

    let source = format!(
        "{bindings} int main() {{ unsafe {{ print(echo(c_init::Two) == c_init::Two); }} return 0; }}"
    );
    let output =
        common::tracked_with_native(&source, "enum init echo(enum init value) { return value; }");
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
    let bindings = common::native_header(&path);
    let source = format!(
        "{bindings} int main() {{ unsafe {{ print((int)byte_echo(Byte::ByteMax)); print((uint64_t)wide_echo(Wide::WideMax)); }} return 0; }}"
    );
    let output = common::tracked_with_native(
        &source,
        "enum Byte byte_echo(enum Byte value) { return value; } enum Wide wide_echo(enum Wide value) { return value; }",
    );
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
        "enum bool { Yes=1 }; enum size_t { Count=2 }; static enum bool boolean_echo(enum bool n){return n;} static enum size_t count_echo(enum size_t n){return n;}\n#define bool unrelated_name\n",
    );
    let source = format!(
        "{}\n#undef bool\nint main(){{print((int)boolean_echo(c_bool::Yes));print((int)count_echo(c_size_t::Count));return 0;}}",
        common::native_header(&path)
    );
    let output = common::tracked_run(&source);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "1\n2\n"
    );
}

#[test]
fn explicit_enum_selection_imports_constant_headers_without_function_or_variable_exports() {
    let path = header(
        "enum Mode { Idle = 0, Active = 42 }; enum Unselected { Other = 3 };\n#define Mode unrelated\n",
    );

    let bindings = common::native_header(&path);

    let c = crustc::compile(&format!(
        "{bindings}\n#undef Mode\nint main() {{ print((int)Mode::Active); return 0; }}"
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
fn native_enum_discovery_reserves_existing_tag_names() {
    let path = header(
        "enum init{First=1};enum c_init{Second=2};static enum init echo(enum init n){return n;}",
    );
    let source = format!(
        "{} int main(){{print((int)echo(c_init_::First));print((int)c_init::Second);return 0;}}",
        common::native_header(&path)
    );
    let output = common::tracked_run(&source);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "1\n2\n"
    );
}

#[test]
fn adjusted_array_parameters_preserve_pointee_const_and_accept_restrict_callback_contracts() {
    let path = header(
        "typedef void CopyFour(int destination[restrict static 4], const int source[static const 4]); int inspect(int destination[restrict static 4], const int source[static const 4], CopyFour* copy);",
    );
    let bindings = common::native_header(&path);

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
    let output = common::tracked_with_native(
        &source,
        "int inspect(int destination[restrict static 4], const int source[static const 4], CopyFour* copy) { copy(destination, source); return destination[3]; }",
    );
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
    // Darwin fortifies this API with compiler intrinsics. Parentheses select
    // the ordinary C function. Keep the macro active to verify that distinction.
    let path = header(
        "#include <stdlib.h>\n#include <string.h>\n#undef memcpy\n#define memcpy(to,from,n) __builtin___memcpy_chk(to,from,n,__builtin_object_size(to,0))\n",
    );
    let bindings = common::native_header(&path);
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
                (memcpy)(&copy[0], &values[0], sizeof(values));
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
    let bindings = common::native_header(&path);
    let vector = include_str!("../../lib/vector.cr")
        .replace("import \"memory.cr\";", include_str!("../../lib/memory.cr"))
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
        let error = crustc::check(&conflicting).unwrap_err();
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
    let bindings = common::native_header(&path);

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
    let generated = crustc::compile(&format!(
        "{} int main(){{return missing(1);}}",
        common::native_header(&path)
    ))
    .unwrap();
    let input = path.with_extension("c");
    std::fs::write(&input, generated).unwrap();
    let error = crustc::native::compile_file(&input, &path.with_extension("out")).unwrap_err();
    assert!(error.contains("missing"), "{error}");
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
    let bindings = common::native_header(&path);
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

    let bindings = common::native_header(&path);
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
        crustc::check(&format!(
            "{bindings} int main() {{ print(counter); return 0; }}"
        ))
        .is_err()
    );
    assert!(
        crustc::check(&format!(
            "{bindings} int main() {{ unsafe {{ limit=1; }} return 0; }}"
        ))
        .is_err()
    );
}

#[test]
fn imports_zero_initialized_header_storage_without_selecting_functions() {
    let path = header("static int value;");

    let bindings = common::native_header(&path);

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
    let root = PathBuf::from(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap(),
    )
    .join("examples/header_only_project/main.cr");
    let output = common::tracked_file(&root, "", true);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n1\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace('\r', ""),
        "drop:bytes:6\n"
    );
}

#[test]
fn native_header_wrappers_cover_extended_c_storage_types_arrays_and_macros() {
    let path = header(include_str!("fixtures/c-wrappers.h"));
    let bindings = common::native_header(&path);
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
                int elements[6]={{1,2,3,10,11,15}};
                print(feature_rows(&elements[0],2));
                const int* row=feature_row(); print(row[0]+row[1]+row[2]);
                print(feature_anonymous(42)); print(feature_extension(42));
                const char* text=feature_text(); const char[]& view=slice(text,5); print(view);
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
        "4\n9\n1\n2\n10\n10\n7.5\n1\n42\n67\n1.5\n42\n42\n42\n42\nCrust\n"
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr).lines().count(), 1);
}
