mod common;
use common::compiler as crust;

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
    let error = crust::check(source).unwrap_err();
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
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
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
    let sources = crust::source::SourceSet::load(&path).unwrap();
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
    let error = crust::check(source).unwrap_err();
    assert!(error.message.contains("unknown variable"), "{error}");
    assert_eq!(error.span.start, source.find("missing").unwrap());
    let source = source.replace("missing", "value");
    let analysis = crust::analyze(&source).unwrap();
    assert!(analysis.symbols.iter().any(|symbol| symbol.name == "value"
        && symbol.location.span.start == source.find("value=").unwrap()));
}

#[test]
fn preprocessing_preserves_standard_boolean_macros_and_reports_unsaved_headers() {
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
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/mixed-unsaved-tests")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&directory).unwrap();
    let header = directory.join("native.h");
    let entry = directory.join("main.cr");
    std::fs::write(&header, "static int value(void){return 42;}\n").unwrap();
    std::fs::write(&entry,"#pragma crust begin_c\n#include \"native.h\"\n#pragma crust end_c\nint main(){return value();}\n").unwrap();
    let result = crust::source::SourceSet::load_with(&entry, |path| {
        if path.ends_with("native.h") {
            return Ok("static int value(void){return 43;}\n".to_owned());
        }
        return std::fs::read_to_string(path);
    });
    let error = result.err().expect("unsaved header must be diagnosed");
    assert!(
        error.to_string().contains("save this included C header"),
        "{error}"
    );
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
    let error = crust::check("\n\n#error deliberate failure\nint main(){return 0;}\n").unwrap_err();
    assert_eq!(error.span.line, 3, "{error}");
    assert!(error.message.contains("deliberate failure"));
}

#[test]
fn native_region_export_headers_require_a_layout_header() {
    let source = "#pragma crust begin_c\nstruct Value {int x;};\n#pragma crust end_c\nextern Value echo(Value value){return value;}\n";
    let error = crust::emit_header(source, crust::ProgramKind::Library).unwrap_err();
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
        .current_dir(env!("CARGO_MANIFEST_DIR"))
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
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
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
    let options = crust::native::Options {
        include_dirs: vec![root.join("first"), root.join("second")],
        ..Default::default()
    };
    let sources =
        crust::source::SourceSet::load_with_options(&root.join("main.cr"), &options, |path| {
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
    crust::native::compile_file_with_options(&generated, &executable, &options).unwrap();
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
