use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    directory: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("project-tests")
            .join(format!(
                "project ü {} {}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&directory).unwrap();
        return Fixture { directory };
    }

    fn write(&self, path: &str, contents: &str) {
        let path = self.directory.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn manifest(&self, targets: &str) {
        self.write(
            "crust.toml",
            &format!("[project]\nname = 'example'\n{targets}"),
        );
    }

    fn invoke(&self, arguments: &[&str]) -> Output {
        return Command::new(env!("CARGO_BIN_EXE_crust"))
            .args(arguments)
            .current_dir(&self.directory)
            .output()
            .unwrap();
    }
}

fn text(output: &Output) -> String {
    return String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
}

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn discovers_projects_from_subdirectories_and_runs_the_named_output() {
    let fixture = Fixture::new();
    fixture.write("src/main.cr", "struct A { int n; static Self create() { return (Self){ .n = 42 }; } }; int main() { A a = A::create(); print(a.n); return 0; }");
    fixture.manifest("[[targets]]\nname = 'hello'\nkind = 'executable'\nentry = 'src/main.cr'\n");
    let output = Command::new(env!("CARGO_BIN_EXE_crust"))
        .arg("run")
        .current_dir(fixture.directory.join("src"))
        .output()
        .unwrap();
    success(&output);
    assert_eq!(text(&output), "42\n");
    assert!(
        fixture
            .directory
            .join("target/crust/hello/hello.c")
            .is_file()
    );
    assert!(
        fixture
            .directory
            .join("target/crust/hello/hello")
            .with_extension(std::env::consts::EXE_EXTENSION)
            .is_file()
    );
    let output = fixture.invoke(&[]);
    success(&output);
    assert_eq!(text(&output), "42\n");
}

#[test]
fn native_sources_includes_objects_and_libraries_link_with_paths_containing_spaces() {
    let fixture = Fixture::new();
    fixture.write("src/main.cr", "extern int from_c(int); extern int from_object(); int main() { unsafe { print(from_c(37)); print(from_object()); } return 0; }");
    fixture.write("native include/answer.h", "#define NATIVE_BONUS 5\n");
    fixture.write(
        "native source/answer.input",
        "#include \"answer.h\"\nint from_c(int value) { return value + NATIVE_BONUS; }\n",
    );
    fixture.write(
        "native source/object.c",
        "int from_object(void) { return 7; }\n",
    );
    fixture.write("native libs/.keep", "");
    let compiler = std::env::var_os("CC").unwrap_or_else(|| {
        if cfg!(windows) {
            "clang".into()
        } else {
            "cc".into()
        }
    });
    let result = Command::new(compiler)
        .args(["-std=c11", "-c"])
        .arg(fixture.directory.join("native source/object.c"))
        .arg("-o")
        .arg(fixture.directory.join("native libs/answer.obj"))
        .output()
        .unwrap();
    success(&result);
    let library = if cfg!(windows) { "kernel32" } else { "m" };
    fixture.manifest(&format!("[[targets]]\nname = 'app'\nkind = 'executable'\nentry = 'src/main.cr'\n[targets.native]\nsources = ['native source/answer.input']\ninclude-dirs = ['native include']\nlibrary-dirs = ['native libs']\nlibraries = ['{library}']\nlink-inputs = ['native libs/answer.obj']\n"));
    let checked = Command::new(env!("CARGO_BIN_EXE_crust"))
        .arg("check")
        .env("CC", "missing-native-test-compiler")
        .current_dir(&fixture.directory)
        .output()
        .unwrap();
    success(&checked);
    assert!(!fixture.directory.join("target").exists());
    let output = fixture.invoke(&["run"]);
    success(&output);
    assert_eq!(text(&output), "42\n7\n");
}

#[test]
fn native_options_are_selected_independently_for_each_target() {
    let fixture = Fixture::new();
    fixture.write(
        "main.cr",
        "extern int native_value(); int main() { unsafe { print(native_value()); } return 0; }",
    );
    fixture.write("first.c", "int native_value(void) { return 1; }");
    fixture.write("second.c", "int native_value(void) { return 2; }");
    fixture.manifest("[[targets]]\nname = 'first'\nkind = 'executable'\nentry = 'main.cr'\n[targets.native]\nsources = ['first.c']\n[[targets]]\nname = 'second'\nkind = 'executable'\nentry = 'main.cr'\n[targets.native]\nsources = ['second.c']\n");
    success(&fixture.invoke(&["build"]));
    for (name, expected) in [("first", "1\n"), ("second", "2\n")] {
        let output = fixture.invoke(&["run", "--target", name]);
        success(&output);
        assert_eq!(text(&output), expected);
    }
}

#[test]
fn opaque_c_handles_have_private_wrappers_and_destroy_once_after_a_move() {
    let fixture = Fixture::new();
    fixture.write(
        "crust.toml",
        include_str!("../examples/opaque_project/crust.toml"),
    );
    fixture.write(
        "src/main.cr",
        include_str!("../examples/opaque_project/src/main.cr"),
    );
    fixture.write(
        "include/native_counter.h",
        include_str!("../examples/opaque_project/include/native_counter.h"),
    );
    fixture.write(
        "native/counter.c",
        include_str!("../examples/opaque_project/native/counter.c"),
    );
    let output = fixture.invoke(&["run"]);
    success(&output);
    assert_eq!(text(&output), "42\ndestroy:42\n");
}

#[test]
fn native_code_calls_crust_callbacks_and_returns_native_function_pointers() {
    let fixture = Fixture::new();
    fixture.write(
        "crust.toml",
        include_str!("../examples/callback_project/crust.toml"),
    );
    fixture.write(
        "src/main.cr",
        include_str!("../examples/callback_project/src/main.cr"),
    );
    fixture.write(
        "include/native_callback.h",
        include_str!("../examples/callback_project/include/native_callback.h"),
    );
    fixture.write(
        "native/callback.c",
        include_str!("../examples/callback_project/native/callback.c"),
    );
    let output = fixture.invoke(&["run"]);
    success(&output);
    assert_eq!(text(&output), "42\n42\n41\n");
}

#[test]
fn native_configuration_checks_paths_kinds_duplicates_and_unknown_fields() {
    let fixture = Fixture::new();
    fixture.write("main.cr", "int main() { return 0; }");
    fixture.write("native/helper.c", "int helper(void) { return 1; }");
    for config in [
        "sources = ['missing.c']",
        "sources = ['native']",
        "sources = ['native/helper.c', 'native/helper.c']",
        "sources = ['native/helper.c']\nlink-inputs = ['native/helper.c']",
        "include-dirs = ['native/helper.c']",
        "library-dirs = ['missing-directory']",
        "link-inputs = ['native']",
        "libraries = ['']",
        "libraries = ['../library']",
        "sources = 'native/helper.c'",
        "flags = ['-O2']",
    ] {
        fixture.manifest(&format!("[[targets]]\nname = 'app'\nkind = 'executable'\nentry = 'main.cr'\n[targets.native]\n{config}\n"));
        let output = fixture.invoke(&["check"]);
        assert!(!output.status.success(), "should reject {config}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("crust.toml"));
    }
}

#[test]
fn build_outputs_preserve_native_sources_and_explicit_link_inputs() {
    for source_input in [true, false] {
        let fixture = Fixture::new();
        fixture.write("main.cr", "int main() { return 0; }");
        let input = if source_input {
            "target/crust/app/app.c".to_owned()
        } else {
            format!("target/crust/app/app{}", std::env::consts::EXE_SUFFIX)
        };
        fixture.write(&input, "native input must be preserved");
        fixture.manifest(&format!("[[targets]]\nname = 'app'\nkind = 'executable'\nentry = 'main.cr'\n[targets.native]\n{} = ['{input}']\n", if source_input { "sources" } else { "link-inputs" }));
        let output = fixture.invoke(&["build"]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("overwrite an input file"));
        assert_eq!(
            fs::read_to_string(fixture.directory.join(input)).unwrap(),
            "native input must be preserved"
        );
    }
}

#[test]
fn projects_support_frontend_commands_without_invoking_a_c_compiler() {
    let fixture = Fixture::new();
    fixture.write("src/main.cr", "int main() { return 0; }");
    fixture.manifest("[[targets]]\nname = 'app'\nkind = 'executable'\nentry = 'src/main.cr'\n");
    for mode in ["check", "tokens", "emit-c"] {
        let output = Command::new(env!("CARGO_BIN_EXE_crust"))
            .arg(mode)
            .env("CC", "missing-crust-test-compiler")
            .current_dir(&fixture.directory)
            .output()
            .unwrap();
        success(&output);
        assert!(!text(&output).is_empty());
    }
    assert!(!fixture.directory.join("target").exists());
}

#[test]
fn builds_all_targets_and_requires_selection_for_running_multiple_targets() {
    let fixture = Fixture::new();
    fixture.write("a/main.cr", "int main() { print(1); return 0; }");
    fixture.write("b/main.cr", "int main() { print(2); return 7; }");
    fixture.manifest("[[targets]]\nname = 'first'\nkind = 'executable'\nentry = 'a/main.cr'\n[[targets]]\nname = 'second'\nkind = 'executable'\nentry = 'b/main.cr'\n");
    success(&fixture.invoke(&["build"]));
    for name in ["first", "second"] {
        assert!(
            fixture
                .directory
                .join(format!("target/crust/{name}/{name}.c"))
                .is_file()
        );
    }
    let output = fixture.invoke(&["run"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--target"));
    let first = fixture.invoke(&["run", "--target", "first"]);
    success(&first);
    assert_eq!(text(&first), "1\n");
    let second = fixture.invoke(&["run", "--target", "second"]);
    assert_eq!(second.status.code(), Some(7));
    assert_eq!(text(&second), "2\n");
    let selected = fixture.invoke(&["check", "--target", "first"]);
    success(&selected);
    assert_eq!(text(&selected).lines().count(), 1);
    assert!(
        !fixture
            .invoke(&["check", "--target", "missing"])
            .status
            .success()
    );
}

#[test]
fn explicit_manifests_and_directories_resolve_entries_relative_to_the_project() {
    let fixture = Fixture::new();
    fixture.write("src/main.cr", "int main() { print(42); return 0; }");
    fixture.manifest("[[targets]]\nname = 'app'\nkind = 'executable'\nentry = 'src/main.cr'\n");
    for path in [
        fixture.directory.clone(),
        fixture.directory.join("crust.toml"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_crust"))
            .args(["run", "--project"])
            .arg(&path)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap();
        success(&output);
        assert_eq!(text(&output), "42\n");
    }
    success(&fixture.invoke(&["check", "crust.toml"]));
    success(&fixture.invoke(&["check", "."]));
}

#[test]
fn rejects_invalid_manifests_and_target_names() {
    let fixture = Fixture::new();
    fixture.write("main.cr", "int main() { return 0; }");
    let valid =
        "[project]\nname='example'\n[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n";
    for contents in [
        String::from("not valid TOML"),
        String::from("[project]\nname='example'\ntargets=[]"),
        String::from("targets=[]\n[project]\nname='example'"),
        valid.replace("name='example'", "name=''"),
        valid.replace("name='app'", "name='../escape'"),
        valid.replace("kind='executable'", "kind='unknown'"),
        valid.replace("entry='main.cr'", "entry='missing.cr'"),
        valid.replace("entry='main.cr'", "entry='.'"),
        format!("{valid}\n[dependencies]\nother={{path='../other'}}"),
        format!("{valid}\n[[targets]]\nname='APP'\nkind='executable'\nentry='main.cr'"),
    ] {
        fixture.write("crust.toml", &contents);
        let error = crust::project::Project::load(&fixture.directory).unwrap_err();
        assert!(error.contains("crust.toml"), "{contents}: {error}");
    }
}

#[test]
fn output_paths_preserve_source_files_for_single_file_and_project_builds() {
    let fixture = Fixture::new();
    let source = "int main() { return 0; }";
    fixture.write("target/crust/conflict.c", source);
    let output = fixture.invoke(&["build", "target/crust/conflict.c"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("overwrite an input"));
    assert_eq!(
        fs::read_to_string(fixture.directory.join("target/crust/conflict.c")).unwrap(),
        source
    );

    fixture.write("main.cr", source);
    fixture.write("target/crust/first/first.c", source);
    fixture.manifest("[[targets]]\nname='first'\nkind='executable'\nentry='main.cr'\n[[targets]]\nname='second'\nkind='executable'\nentry='target/crust/first/first.c'");
    let output = fixture.invoke(&["build"]);
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(fixture.directory.join("target/crust/first/first.c")).unwrap(),
        source
    );
}

#[test]
fn project_diagnostics_keep_the_source_filename_and_location() {
    let fixture = Fixture::new();
    fixture.write("src/main.cr", "int main() {\nreturn missing;\n}");
    fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='src/main.cr'");
    let output = fixture.invoke(&["check"]);
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("main.cr:2:8"), "{error}");
    assert!(error.contains("unknown variable"), "{error}");
}

#[test]
fn project_imports_build_and_report_errors_in_dependencies() {
    let fixture = Fixture::new();
    fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='src/main.cr'");
    fixture.write(
        "src/main.cr",
        "import \"helper.cr\"; int main() { print(answer()); return 0; }",
    );
    fixture.write("src/helper.cr", "int answer() { return 42; }");
    let output = fixture.invoke(&["run"]);
    success(&output);
    assert_eq!(text(&output), "42\n");
    fixture.write("src/helper.cr", "int answer() {\n    return missing;\n}");
    let output = fixture.invoke(&["check"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("helper.cr:2:12"));
}

#[test]
fn generated_outputs_preserve_imported_sources_in_selected_and_other_targets() {
    let fixture = Fixture::new();
    let source = "int helper() { return 0; }";
    fixture.write("target/crust/main.c", source);
    fixture.write(
        "main.cr",
        "import \"target/crust/main.c\"; int main() { return helper(); }",
    );
    let output = fixture.invoke(&["build", "main.cr"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("overwrite an input"));
    assert_eq!(
        fs::read_to_string(fixture.directory.join("target/crust/main.c")).unwrap(),
        source
    );

    fixture.write("main.cr", "int main() { return 0; }");
    fixture.write(
        "other.cr",
        "import \"target/crust/first/first.c\"; int main() { return helper(); }",
    );
    fixture.write("target/crust/first/first.c", source);
    fixture.manifest("[[targets]]\nname='first'\nkind='executable'\nentry='main.cr'\n[[targets]]\nname='other'\nkind='executable'\nentry='other.cr'");
    let output = fixture.invoke(&["build", "--target", "first"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("overwrite an input"));
    assert_eq!(
        fs::read_to_string(fixture.directory.join("target/crust/first/first.c")).unwrap(),
        source
    );
}

#[test]
fn native_variadic_functions_receive_promoted_wide_and_function_pointer_arguments() {
    let fixture = Fixture::new();
    fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nsources=['native.c']\n");
    fixture.write(
        "native.c",
        r#"
        #include <stdarg.h>
        #include <stdint.h>
        int sum_promoted(int count, ...) {
            va_list args; va_start(args, count); int sum = 0;
            for (int i = 0; i < count; ++i) { sum += va_arg(args, int); }
            va_end(args); return sum;
        }
        int64_t sum_wide(int count, ...) {
            va_list args; va_start(args, count); int64_t sum = 0;
            for (int i = 0; i < count; ++i) { sum += va_arg(args, int64_t); }
            va_end(args); return sum;
        }
        uint64_t unsigned_value(int ignored, ...) {
            va_list args; va_start(args, ignored); uint64_t value = va_arg(args, uint64_t);
            va_end(args); return value;
        }
        typedef int (*Reader)(int*);
        int invoke(int ignored, ...) {
            va_list args; va_start(args, ignored); Reader read = va_arg(args, Reader);
            int *value = va_arg(args, int*); va_end(args); return read(value);
        }
        typedef int (*Sum)(int, ...);
        Sum get_sum(void) { return sum_promoted; }
    "#,
    );
    fixture.write(
        "main.cr",
        r#"
        extern int sum_promoted(int count, ...);
        extern int64_t sum_wide(int count, ...);
        extern uint64_t unsigned_value(int ignored, ...);
        extern int invoke(int ignored, ...);
        typedef int (*Sum)(int, ...);
        extern Sum get_sum();
        unsafe int read(int* value) { return *value; }
        enum Small : uint8_t { Ready = 7 };
        int main() {
            uint8_t byte = 255; uint16_t half = 65535; bool flag = true;
            int64_t wide = 2147483648; uint64_t largest = 18446744073709551615;
            int value = 42;
            unsafe {
                print(sum_promoted(0)); print(sum_promoted(4, byte, half, flag, Small::Ready));
                print(sum_wide(2, wide, (int64_t)1)); print(unsigned_value(0, largest));
                print(invoke(0, read, &value)); print(get_sum()(2, (char)'a', (uint8_t)1));
            }
            return 0;
        }
    "#,
    );
    let output = fixture.invoke(&["run"]);
    success(&output);
    assert_eq!(
        text(&output),
        "0\n65798\n2147483649\n18446744073709551615\n42\n98\n"
    );
}

#[test]
fn native_unions_match_value_pointer_layout_and_opaque_tag_abis() {
    let fixture = Fixture::new();
    fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nsources=['native.c']\n");
    fixture.write("native.c", r#"
        #include <stdint.h>
        #include <stddef.h>
        union Native { int32_t narrow; int64_t wide; uint8_t bytes[8]; };
        union Native native_make(int64_t value) { union Native result = { .wide = value }; return result; }
        int64_t native_read(union Native value) { return value.wide; }
        int64_t native_pointer(union Native *value) { return value->wide; }
        size_t native_size(void) { return sizeof(union Native); }
        size_t native_align(void) { return _Alignof(union Native); }
        union Opaque { uint64_t value; };
        static union Opaque opaque = { .value = 13 };
        union Opaque *opaque_get(void) { return &opaque; }
        uint64_t opaque_read(union Opaque *value) { return value->value; }
    "#);
    fixture.write("main.cr", r#"
        union Native { int narrow; int64_t wide; uint8_t bytes[8]; };
        union Opaque;
        extern Native native_make(int64_t value);
        extern int64_t native_read(Native value);
        extern int64_t native_pointer(Native* value);
        extern size_t native_size();
        extern size_t native_align();
        extern union Opaque* opaque_get();
        extern uint64_t opaque_read(union Opaque* value);
        int main() {
            unsafe {
                Native value = { .wide = 2147483648 }; print(native_read(value)); print(native_pointer(&value));
                Native next = native_make(42); print(next.wide);
                print(sizeof(Native) == native_size()); print(alignof(Native) == native_align());
                print(opaque_read(opaque_get()));
            }
            return 0;
        }
    "#);
    let output = fixture.invoke(&["run"]);
    success(&output);
    assert_eq!(text(&output), "2147483648\n2147483648\n42\n1\n1\n13\n");
}

#[test]
fn rejects_incomplete_duplicate_and_conflicting_cli_options() {
    let fixture = Fixture::new();
    for arguments in [
        vec!["--project"],
        vec!["--target"],
        vec!["--unknown"],
        vec!["check", "one.cr", "two.cr"],
        vec!["check", "one.cr", "--project", "."],
        vec!["check", "one.cr", "--target", "app"],
        vec!["check", "--project", ".", "--project", "."],
        vec!["check", "--target", "a", "--target", "b"],
    ] {
        assert!(
            !fixture.invoke(&arguments).status.success(),
            "{arguments:?}"
        );
    }
    assert!(crust::project::Project::discover(Path::new(".")).is_err());
}
