mod common;
use common::compiler as crustc;

#[test]
fn native_region_marker_errors_point_to_the_pragma_instead_of_the_c_header() {
    for (pragma, expected) in [
        ("#pragma begin_c", "#pragma crust begin_c"),
        ("#pragma end_c", "#pragma crust end_c"),
        ("#pragma crust begin", "unknown Crust pragma"),
    ] {
        let source =
            format!("// header\r\n  {pragma}\r\n#include <stdio.h>\r\nint main(){{return 0;}}\r\n");
        let diagnostic = crustc::check(&source).unwrap_err();
        assert!(diagnostic.message.contains(expected), "{diagnostic}");
        assert_eq!(diagnostic.span.line, 2);
        assert_eq!(diagnostic.span.column, 3);
        assert_eq!(&source[diagnostic.span.start..diagnostic.span.end], pragma);
    }
    crustc::check("#if 0\n#pragma begin_c\n#pragma crust bad\n#endif\nint main(){return 0;}\n")
        .unwrap();
}

#[test]
fn stdio_header_example_imports_printf_and_runs_through_the_cli() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_crust"))
        .args(["run", "examples/stdio.cr"])
        .current_dir(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap(),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "hello\n"
    );
}

#[test]
fn cc_preprocesses_crust_generics_macros_and_conditionals() {
    let output = common::tracked_run(
        r#"
        #define TAKE(value) move(value)
        #define COUNT 4
        #define JOIN(a,b) a ## b
        #define TEXT(value) #value
        #if COUNT != 4
        this is deliberately invalid Crust
        #endif
        int main(){
            Box<int> owner=Box<int>::create(42);
            Box<int> second=TAKE(owner);
            int values[COUNT]={1,2,3,4};
            int JOIN(ans,wer)=second.get()+values[0]-1;
            print(answer); print(TEXT(Crust)); return 0;
        }
    "#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\nCrust\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace('\r', ""),
        "drop:42\n"
    );
    let source = "#define TAKE(value) move(value)\nint main(){Box<int> owner=Box<int>::create(42);Box<int> second=TAKE(owner);print(owner.get());return 0;}";
    let error = crustc::check(source).unwrap_err();
    assert!(error.message.contains("moved"), "{error}");
    assert_eq!(error.span.line, 2);
}

#[test]
fn native_regions_share_macros_and_call_crust_exports() {
    let source = r#"
        #define FACTOR 3
        #pragma crust begin_c
        extern int source_increment(int);
        static int triple(int value){return value*FACTOR;}
        static int call_crust(int value){return source_increment(value);}
        #if 0
        } // Preprocessing owns inactive braces.
        #endif
        #pragma crust end_c
        extern int source_increment(int value){return value+1;}
        int main(){print(triple(14));print(call_crust(41));return 0;}
    "#;
    let output = common::tracked_run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n42\n"
    );
}

#[test]
fn native_headers_supply_types_functions_and_callbacks_without_binding_files() {
    let directory = std::path::Path::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap(),
    )
    .join("target/mixed-source-tests/project ü")
    .join(std::process::id().to_string());
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("native.h"),r#"
        #ifndef CRUST_NATIVE_TEST_H
        #define CRUST_NATIVE_TEST_H
        #include <stdint.h>
        #include <stdio.h>
        struct Point{int x;int y;};
        static struct Point apply(struct Point (*callback)(struct Point),struct Point p){return callback(p);}
        #define VALUE 42
        #endif
    "#).unwrap();
    let path = directory.join("main.cr");
    std::fs::write(&path,r#"
        #pragma crust begin_c
        #include "native.h"
        #include "native.h"
        #pragma crust end_c
        Point next(Point value){value.x++;return value;}
        int main(){Point point={.x=40,.y=1};unsafe{Point result=apply(&next,point);print(result.x+result.y);}print(VALUE);return 0;}
    "#).unwrap();
    let sources = crustc::source::SourceSet::load(&path).unwrap();
    let generated = sources.compile().unwrap();
    let output = common::tracked_generated_c(&generated, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n42\n"
    );
    let analysis = sources.analyze().unwrap();
    assert!(analysis.files.iter().any(|file| {
        file.path
            .as_ref()
            .is_some_and(|path| path.ends_with("native.h"))
    }));
}

#[test]
fn preprocessed_diagnostics_and_analysis_use_original_source_positions() {
    let source = "#define LONG_VALUE (10+20+12)\nint main(){int value=LONG_VALUE; print(missing);return 0;}\n";
    let error = crustc::check(source).unwrap_err();
    assert!(error.message.contains("unknown variable"), "{error}");
    assert_eq!(error.span.start, source.find("missing").unwrap());
    let source = source.replace("missing", "value");
    let analysis = crustc::analyze(&source).unwrap();
    assert!(analysis.symbols.iter().any(|symbol| symbol.name == "value"
        && symbol.location.span.start == source.find("value=").unwrap()));
}

#[test]
fn preprocessing_preserves_standard_boolean_macros_and_snapshots_unsaved_headers() {
    let output = common::tracked_run(
        r#"
#pragma crust begin_c
#include <stdbool.h>
static bool native_true(void){return true;}
#pragma crust end_c
int main(){bool value=true;print(value && native_true());return 0;}
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let directory = std::path::Path::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap(),
    )
    .join("target/mixed-unsaved-tests")
    .join(std::process::id().to_string());
    std::fs::create_dir_all(&directory).unwrap();
    let header = directory.join("native.h");
    let entry = directory.join("main.cr");
    std::fs::write(&header, "static int value(void){return 42;}\n").unwrap();
    std::fs::write(&entry,"#pragma crust begin_c\n#include \"native.h\"\n#pragma crust end_c\nint main(){return value();}\n").unwrap();
    let result = crustc::source::SourceSet::load_with(&entry, |path| {
        if path.ends_with("native.h") {
            return Ok("static int value(void){return 43;}\n".to_owned());
        }
        return std::fs::read_to_string(path);
    });
    let sources = result.unwrap();
    std::fs::write(&header, "static int value(void){return 99;}\n").unwrap();
    let output = common::tracked_generated_c(&sources.compile().unwrap(), false);
    assert_eq!(output.status.code(), Some(43));
}

#[test]
fn native_regions_preserve_promoted_groups_bitfields_and_enum_constants() {
    let output = common::tracked_run(
        r#"
#pragma crust begin_c
struct Native { union { int x; int y; }; unsigned int flags:3; };
enum Mode { NativeAnswer = 42 };
static int native_read(struct Native value){return value.x + value.flags;}
#pragma crust end_c
int main(){Native value={.x=40,.flags=2};print(native_read(value));print((int)NativeAnswer);return 0;}
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n42\n"
    );
}

#[test]
fn preprocessor_errors_keep_the_directive_location() {
    let error =
        crustc::check("\n\n#error deliberate failure\nint main(){return 0;}\n").unwrap_err();
    assert_eq!(error.span.line, 3, "{error}");
    assert!(error.message.contains("deliberate failure"));
}

#[test]
fn native_region_export_headers_require_a_layout_header() {
    let source = "#pragma crust begin_c\nstruct Value {int x;};\n#pragma crust end_c\nextern Value echo(Value value){return value;}\n";
    let error = crustc::emit_header(source, crustc::ProgramKind::Library).unwrap_err();
    assert!(error.message.contains("extern header"), "{error}");
}

#[test]
fn native_regions_accept_crlf_and_bare_cr_line_endings() {
    let source = "#define VALUE 42\n# pragma crust begin_c\nstatic int native_answer(void){return VALUE;}\n# pragma crust end_c\nint main(){print(native_answer());return 0;}\n";
    for newline in ["\r\n", "\r"] {
        let output = common::tracked_run(&source.replace('\n', newline));
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
}

#[test]
fn cli_compiles_native_standard_headers_before_the_generated_runtime() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_crust"))
        .args(["run", "examples/mixed.cr", "--release"])
        .current_dir(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap(),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n42\n"
    );
}

#[test]
fn quoted_search_path_headers_preserve_include_next() {
    let root = std::path::Path::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap(),
    )
    .join("target/mixed-search-tests")
    .join(std::process::id().to_string());
    std::fs::create_dir_all(root.join("first")).unwrap();
    std::fs::create_dir_all(root.join("second")).unwrap();
    std::fs::write(
        root.join("first/api.h"),
        "#ifndef FIRST_API\n#define FIRST_API\n#include_next \"api.h\"\n#endif\n",
    )
    .unwrap();
    std::fs::write(root.join("second/api.h"),"#ifndef SECOND_API\n#define SECOND_API\nstatic int native_answer(void){return 42;}\n#endif\n").unwrap();
    std::fs::write(root.join("main.cr"),"#pragma crust begin_c\n#include \"api.h\"\n#include \"api.h\"\n#pragma crust end_c\nint main(){print(native_answer());return 0;}\n").unwrap();
    let options = crustc::native::Options {
        include_dirs: vec![root.join("first"), root.join("second")],
        ..Default::default()
    };
    let sources =
        crustc::source::SourceSet::load_with_options(&root.join("main.cr"), &options, |path| {
            std::fs::read_to_string(path)
        })
        .unwrap();
    let generated = root.join("output.c");
    std::fs::write(&generated, sources.compile().unwrap()).unwrap();
    let executable = root.join(if cfg!(windows) {
        "output.exe"
    } else {
        "output"
    });
    crustc::native::compile_file_with_options(&generated, &executable, &options).unwrap();
    let output = std::process::Command::new(executable).output().unwrap();
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

#[test]
fn replay_preserves_counters_and_header_state_across_imports() {
    let root = std::path::Path::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap(),
    )
    .join("target/replay-import-tests")
    .join(std::process::id().to_string());
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("native.h"),"#pragma once\n#include <stdint.h>\nstruct Shared {int value;};\nstatic int header_number(void){return __COUNTER__;}\n").unwrap();
    std::fs::write(root.join("dependency.cr"),"#pragma crust begin_c\n#include \"native.h\"\nstatic int dependency_number(void){return __COUNTER__;}\n#pragma crust end_c\nconst int dependency_counter=__COUNTER__;\nstatic int local(){return 40;}\nint dependency(){return local()+dependency_number()+dependency_counter;}\n").unwrap();
    std::fs::write(root.join("main.cr"),"import \"dependency.cr\";\nimport \"dependency.cr\";\nconst int root_counter=__COUNTER__;\n#pragma crust begin_c\n#include \"native.h\"\nstatic int root_number(void){return __COUNTER__;}\n#pragma crust end_c\nstatic int local(){return 2;}\nint main(){print(dependency());print(root_counter);print(root_number());print(header_number());print(local());return 0;}\n").unwrap();
    let sources = crustc::source::SourceSet::load(&root.join("main.cr")).unwrap();
    assert!(
        sources
            .files()
            .iter()
            .all(|file| !file.path.to_string_lossy().contains("crust-source-"))
    );
    assert!(
        sources
            .files()
            .iter()
            .any(|file| file.path == root.join("dependency.cr").canonicalize().unwrap())
    );
    let output = common::tracked_generated_c(&sources.compile().unwrap(), false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "43\n3\n4\n0\n2\n"
    );
}

#[test]
fn replay_preserves_first_expansion_with_generated_runtime_macros() {
    let source = "const int crust_number=__COUNTER__;\n#pragma crust begin_c\n#include <stdio.h>\nstatic int native_number(void){return __COUNTER__;}\n#pragma crust end_c\nint main(){print(crust_number);print(native_number());return 0;}\n";
    let output = common::tracked_run(source);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "0\n1\n"
    );
}

#[test]
fn replay_snapshots_macro_only_headers_and_keeps_original_paths() {
    let root = std::path::Path::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap(),
    )
    .join("target/replay-macro-snapshot")
    .join(std::process::id().to_string());
    std::fs::create_dir_all(&root).unwrap();
    let entry = root.join("main.cr");
    let header = root.join("macros.h");
    std::fs::write(&header, "#pragma once\n#define ANSWER 1\n").unwrap();
    std::fs::write(&entry,"#pragma crust begin_c\n#include \"macros.h\"\n#pragma crust end_c\nint main(){return ANSWER;}\n").unwrap();
    let sources = crustc::source::SourceSet::load_with(&entry, |path| {
        if path.ends_with("macros.h") {
            Ok("#pragma once\n#define ANSWER 42\n".to_owned())
        } else {
            std::fs::read_to_string(path)
        }
    })
    .unwrap();
    assert!(
        sources
            .files()
            .iter()
            .all(|file| !file.path.to_string_lossy().contains("crust-source-"))
    );
    std::fs::write(&header, "#pragma once\n#define ANSWER 99\n").unwrap();
    let generated = sources.compile().unwrap();
    assert!(!generated.lines().any(|line| line.starts_with("#line ")));
    drop(sources);
    assert_eq!(
        common::tracked_generated_c(&generated, false).status.code(),
        Some(42)
    );
}

#[test]
fn replay_uses_the_target_kind_and_optimization_profile_for_both_languages() {
    let root = std::path::Path::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap(),
    )
    .join("target/replay-profile-tests")
    .join(std::process::id().to_string());
    std::fs::create_dir_all(&root).unwrap();
    let entry = root.join("main.cr");
    std::fs::write(&entry,"#ifdef __OPTIMIZE__\n#define LEVEL 1\n#else\n#define LEVEL 0\n#endif\n#ifdef CRUST_BUILD_SHARED\n#define SHARED 2\n#else\n#define SHARED 0\n#endif\n#pragma crust begin_c\nstatic int native_flags(void){return LEVEL+SHARED;}\n#pragma crust end_c\nint main(){return native_flags()+LEVEL+SHARED;}\n").unwrap();
    for (kind, profile, expected) in [
        (
            crustc::project::TargetKind::Executable,
            crustc::native::Profile::Debug,
            0,
        ),
        (
            crustc::project::TargetKind::Executable,
            crustc::native::Profile::Release,
            2,
        ),
        (
            crustc::project::TargetKind::SharedLibrary,
            crustc::native::Profile::Release,
            6,
        ),
    ] {
        let options = crustc::native::Options {
            profile,
            ..Default::default()
        };
        let sources = crustc::source::SourceSet::load_for_target(&entry, &options, kind, |path| {
            std::fs::read_to_string(path)
        })
        .unwrap();
        let output = common::tracked_generated_c(&sources.compile().unwrap(), false);
        assert_eq!(output.status.code(), Some(expected), "{output:?}");
    }
}

#[test]
fn conditional_imports_use_shared_macros_and_ignore_inactive_syntax() {
    let root = std::path::Path::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap(),
    )
    .join("target/replay-conditional-tests")
    .join(std::process::id().to_string());
    std::fs::create_dir_all(&root).unwrap();
    let entry = root.join("main.cr");
    std::fs::write(
        root.join("dependency.cr"),
        "#define AFTER_IMPORT 2\nint dependency(){return BEFORE_IMPORT;}\n",
    )
    .unwrap();
    std::fs::write(&entry,"#define BEFORE_IMPORT 40\n#define MODULE \"dependency.cr\"\n#if 0\ninvalid @ { syntax\nimport \"/does-not-exist.cr\";\n#endif\nimport MODULE;\nint main(){return dependency()+AFTER_IMPORT;}\n").unwrap();
    let sources = crustc::source::SourceSet::load(&entry).unwrap();
    assert_eq!(
        common::tracked_generated_c(&sources.compile().unwrap(), false)
            .status
            .code(),
        Some(42)
    );
    std::fs::write(
        &entry,
        "#define VALUE 1\nint main(){import \"dependency.cr\";return VALUE;}\n",
    )
    .unwrap();
    let error = crustc::source::SourceSet::load(&entry).err().unwrap();
    assert!(error.diagnostic.message.contains("top-level"), "{error}");
}

#[test]
fn source_functions_take_precedence_over_incidental_header_declarations() {
    let output = common::tracked_run(
        r#"
#pragma crust begin_c
char* index(const char*,int);
int identity(int);
struct SameName {int value;};
#pragma crust end_c
int index(){return 42;}
T identity<T>(T value){return value;}
int SameName(){return 7;}
int main(){SameName value={.value=identity<int>(index())};print(value.value);print(SameName());return 0;}
"#,
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n7\n"
    );
}

#[test]
fn explicitly_unsafe_header_declarations_keep_their_caller_contract() {
    let source = "#pragma crust begin_c\nstatic int native_value(int n){return n;}\n#pragma crust end_c\nunsafe extern int native_value(int n);\nint main(){return native_value(42);}\n";
    assert!(
        crustc::check(source)
            .unwrap_err()
            .message
            .contains("unsafe")
    );
    let output = common::tracked_run(&source.replace(
        "return native_value(42);",
        "unsafe{return native_value(42);}",
    ));
    assert_eq!(output.status.code(), Some(42), "{output:?}");
}

#[test]
fn generated_headers_accept_synthetic_line_filenames_without_opening_them() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/cgo-line-labels")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("generated.h"),
        r#"
#line 1 "cgo-builtin-export-prolog"
typedef long long GoInt64;
#line 1 "cgo-gcc-export-header-prolog"
#include "tail.h"
#line 77 "virtual-generated-export.h"
static inline GoInt64 cgo_answer(GoInt64 value) { return value + CGO_OFFSET; }
"#,
    )
    .unwrap();
    std::fs::write(root.join("tail.h"), "#define CGO_OFFSET 1\n").unwrap();
    let entry = root.join("main.cr");
    std::fs::write(&entry, "#pragma crust begin_c\n#include \"generated.h\"\n#pragma crust end_c\nint main(){ print(cgo_answer(41LL)); return 0; }\n").unwrap();
    let sources = crustc::source::SourceSet::load_with(&entry, |path| {
        assert!(
            !matches!(
                path.file_name().and_then(|name| name.to_str()),
                Some(
                    "cgo-builtin-export-prolog"
                        | "cgo-gcc-export-header-prolog"
                        | "virtual-generated-export.h"
                )
            ),
            "attempted to open logical filename: {}",
            path.display()
        );
        std::fs::read_to_string(path)
    })
    .unwrap();
    let output = common::tracked_generated_c(&sources.compile().unwrap(), false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n"
    );

    std::fs::write(
        root.join("bad.h"),
        "#line 37 \"virtual-generated-error.h\"\nmissing_c_type cgo_bad(void);\n",
    )
    .unwrap();
    std::fs::write(
        &entry,
        "#pragma crust begin_c\n#include \"bad.h\"\n#pragma crust end_c\nint main(){return 0;}\n",
    )
    .unwrap();
    let error = crustc::source::SourceSet::load_with(&entry, |path| {
        assert!(!path.ends_with("virtual-generated-error.h"));
        std::fs::read_to_string(path)
    })
    .err()
    .expect("invalid native declaration should report a C diagnostic");
    assert!(
        error.diagnostic.message.contains("missing_c_type"),
        "{error}"
    );
    assert_eq!(error.diagnostic.span.line, 37, "{error}");
}
