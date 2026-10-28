mod common;
use common::compiler as crust;

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
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("project-tests");
        fs::create_dir_all(&root).unwrap();
        loop {
            let directory = root.join(format!(
                "project ü {} {}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&directory) {
                Ok(()) => return Fixture { directory },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("could not create project fixture: {error}"),
            }
        }
    }

    fn write(&self, path: &str, contents: &str) {
        let path = self.directory.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let contents = if path.extension().is_some_and(|extension| extension == "cr")
            && !contents.contains("/box.cr\"")
        {
            common::compiler::fixture(contents)
        } else {
            contents.to_owned()
        };
        fs::write(path, contents).unwrap();
    }

    fn manifest(&self, targets: &str) {
        self.write(
            "crust.toml",
            &format!("[project]\nname = 'example'\n{targets}"),
        );
    }

    fn box_library(&self) {
        for (name, source) in [
            ("box", include_str!("../lib/box.cr")),
            ("memory", include_str!("../lib/memory.cr")),
            ("allocation", include_str!("../lib/allocation.cr")),
            ("result", include_str!("../lib/result.cr")),
        ] {
            self.write(&format!("lib/{name}.cr"), source);
        }
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
        "status: {:?}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn executable(fixture: &Fixture) -> PathBuf {
    return fixture
        .directory
        .join("target/crust/app/app")
        .with_extension(std::env::consts::EXE_EXTENSION);
}

fn assert_no_build_staging(fixture: &Fixture) {
    assert!(
        fs::read_dir(fixture.directory.join("target/crust/app"))
            .unwrap()
            .all(|entry| {
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("crust-build-")
            })
    );
}

#[test]
fn static_libraries_supply_transitive_native_link_requirements_and_rebuild_consumers() {
    let fixture = Fixture::new();
    fixture.write(
        "vendor libs/vendor.c",
        "int vendor_answer(void) { return 42; }",
    );
    let vendor = fixture
        .directory
        .join("vendor libs")
        .join(if cfg!(windows) {
            "vendor.lib"
        } else {
            "libvendor.a"
        });
    crust::native::compile_static_library(
        &fixture.directory.join("vendor libs/vendor.c"),
        &vendor,
        &Default::default(),
    )
    .unwrap();
    fixture.write(
        "base.cr",
        "extern int vendor_answer(); extern int base_answer() { return vendor_answer(); }",
    );
    fixture.write(
        "bridge.cr",
        "extern int base_answer(); extern int answer() { return base_answer(); }",
    );
    fixture.write(
        "main.cr",
        "extern int answer(); int main() { unsafe { print(answer()); } return 0; }",
    );
    fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nlink-targets=['bridge']\n[[targets]]\nname='bridge'\nkind='static-library'\nentry='bridge.cr'\n[targets.native]\nlink-targets=['base']\n[[targets]]\nname='base'\nkind='static-library'\nentry='base.cr'\n[targets.native]\nlibrary-dirs=['vendor libs']\nlibraries=['vendor']\n");
    let project = crust::project::Project::load(&fixture.directory).unwrap();
    let app = project.select(Some("app"), true).unwrap()[0];
    let native = project.resolved_native(app).unwrap();
    assert_eq!(native.libraries, vec!["vendor"]);
    assert_eq!(
        native.library_dirs,
        vec![
            fixture
                .directory
                .join("vendor libs")
                .canonicalize()
                .unwrap()
        ]
    );
    let output = fixture.invoke(&["run", "--target", "app"]);
    success(&output);
    assert_eq!(text(&output), "42\n");
    fixture.write(
        "vendor libs/vendor.c",
        "int vendor_answer(void) { return 43; }",
    );
    crust::native::compile_static_library(
        &fixture.directory.join("vendor libs/vendor.c"),
        &vendor,
        &Default::default(),
    )
    .unwrap();
    let output = fixture.invoke(&["run", "--target", "app"]);
    success(&output);
    assert_eq!(text(&output), "43\n");
}

#[test]
fn transitive_library_flags_preserve_order_repetitions_and_unique_search_directories() {
    let fixture = Fixture::new();
    fixture.write("main.cr", "int main() { return 0; }");
    fixture.write("library.cr", "extern int answer() { return 42; }");
    fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nlink-targets=['middle']\nlibraries=['first']\nlibrary-dirs=['.']\n[[targets]]\nname='middle'\nkind='static-library'\nentry='library.cr'\n[targets.native]\nlink-targets=['base']\nlibraries=['second','first']\nlibrary-dirs=['.']\n[[targets]]\nname='base'\nkind='static-library'\nentry='library.cr'\n[targets.native]\nlibraries=['third']\n");
    let project = crust::project::Project::load(&fixture.directory).unwrap();
    let app = project.select(Some("app"), true).unwrap()[0];
    let native = project.resolved_native(app).unwrap();
    assert_eq!(native.libraries, vec!["first", "second", "first", "third"]);
    assert_eq!(native.library_dirs.len(), 1);
    success(&fixture.invoke(&["check"]));
}

#[test]
fn sanitizer_settings_propagate_from_native_library_dependencies() {
    let fixture = Fixture::new();
    fixture.write("main.cr", "int main() { return 0; }");
    fixture.write("lib.cr", "extern int value() { return 1; }");
    fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nlink-targets=['library']\n[[targets]]\nname='library'\nkind='static-library'\nentry='lib.cr'\n[targets.native]\nsanitizers=['address','undefined']\n");
    let project = crust::project::Project::load(&fixture.directory).unwrap();
    let app = project.select(Some("app"), true).unwrap()[0];
    let options = project.resolved_native(app).unwrap();
    assert_eq!(
        options.sanitizers,
        vec![
            crust::native::Sanitizer::Address,
            crust::native::Sanitizer::Undefined
        ]
    );
}

fn compiler_stub(fixture: &Fixture, writes_output: bool, code: u8) -> PathBuf {
    let name = if cfg!(windows) {
        "compiler.cmd"
    } else {
        "compiler.sh"
    };
    let source = if cfg!(windows) {
        if writes_output {
            format!(
                "@echo off\r\n:args\r\nif \"%~1\"==\"\" exit /b {code}\r\nif \"%~1\"==\"-o\" goto output\r\nshift\r\ngoto args\r\n:output\r\nshift\r\n> \"%~1\" echo invalid-executable\r\nexit /b {code}\r\n"
            )
        } else {
            format!("@echo off\r\nexit /b {code}\r\n")
        }
    } else if writes_output {
        format!(
            "#!/bin/sh\nwhile [ \"$#\" -gt 0 ]; do\nif [ \"$1\" = \"-o\" ]; then\nshift\nprintf 'invalid-executable' > \"$1\"\nexit {code}\nfi\nshift\ndone\nexit {code}\n"
        )
    } else {
        format!("#!/bin/sh\nexit {code}\n")
    };
    fixture.write(name, &source);
    let path = fixture.directory.join(name);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    return path;
}

#[test]
fn executable_rebuilds_preserve_the_previous_program_after_compile_and_link_failures() {
    let fixture = Fixture::new();
    fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nsources=['native.c']\n");
    fixture.write(
        "main.cr",
        "extern int answer(); int main() { unsafe { print(answer()); } return 0; }",
    );
    fixture.write("native.c", "int answer(void) { return 42; }");
    success(&fixture.invoke(&["build"]));
    let binary = executable(&fixture);
    let previous = fs::read(&binary).unwrap();
    for source in [
        "this is invalid C",
        "extern int missing(void); int answer(void) { return missing(); }",
    ] {
        fixture.write("native.c", source);
        let output = fixture.invoke(&["build"]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("C compiler failed"));
        assert_eq!(fs::read(&binary).unwrap(), previous);
        assert_eq!(text(&Command::new(&binary).output().unwrap()), "42\n");
        assert_no_build_staging(&fixture);
    }
    fixture.write("native.c", "int answer(void) { return 7; }");
    success(&fixture.invoke(&["build"]));
    assert_eq!(text(&Command::new(&binary).output().unwrap()), "7\n");
    assert_no_build_staging(&fixture);
}

#[test]
fn missing_native_output_is_an_error_instead_of_running_a_stale_executable() {
    for existing in [false, true] {
        let fixture = Fixture::new();
        fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n");
        fixture.write("main.cr", "int main() { print(42); return 0; }");
        let binary = executable(&fixture);
        let previous = if existing {
            success(&fixture.invoke(&["build"]));
            Some(fs::read(&binary).unwrap())
        } else {
            None
        };
        fixture.write("main.cr", "int main() { print(7); return 0; }");
        let output = Command::new(env!("CARGO_BIN_EXE_crust"))
            .arg("run")
            .current_dir(&fixture.directory)
            .env("CC", compiler_stub(&fixture, false, 0))
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("did not produce an executable"));
        assert!(output.stdout.is_empty());
        if let Some(previous) = previous {
            assert_eq!(fs::read(&binary).unwrap(), previous);
        } else {
            assert!(!binary.exists());
        }
        assert_no_build_staging(&fixture);
    }
}

#[test]
fn invalid_and_partial_native_outputs_restore_previous_executables_or_remove_failed_files() {
    for code in [0, 1] {
        for existing in [false, true] {
            let fixture = Fixture::new();
            fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n");
            fixture.write("main.cr", "int main() { print(42); return 0; }");
            let binary = executable(&fixture);
            let previous = if existing {
                success(&fixture.invoke(&["build"]));
                Some(fs::read(&binary).unwrap())
            } else {
                None
            };
            let output = Command::new(env!("CARGO_BIN_EXE_crust"))
                .arg("build")
                .current_dir(&fixture.directory)
                .env("CC", compiler_stub(&fixture, true, code))
                .output()
                .unwrap();
            assert!(!output.status.success());
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(
                error.contains(if code == 0 {
                    "invalid native file signature"
                } else {
                    "C compiler failed"
                }),
                "{error}"
            );
            if let Some(previous) = previous {
                assert_eq!(fs::read(&binary).unwrap(), previous);
            } else {
                assert!(!binary.exists());
            }
            assert_no_build_staging(&fixture);
        }
    }
}

#[test]
fn native_driver_accepts_executable_output_paths_without_extensions() {
    let fixture = Fixture::new();
    fixture.write("native.c", "int main(void) { return 0; }");
    let output = fixture.directory.join("program");
    crust::native::compile_file(&fixture.directory.join("native.c"), &output).unwrap();
    assert!(output.is_file());
}

#[test]
fn project_metadata_reports_target_kinds_links_and_output_paths_without_compilation() {
    let fixture = Fixture::new();
    fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nlink-targets=['math']\n[[targets]]\nname='math'\nkind='static-library'\nentry='library.cr'\n");
    // Metadata remains available while source files are being edited.
    fixture.write("main.cr", "incomplete source");
    fixture.write("library.cr", "import \"missing.cr\";");
    let output = Command::new(env!("CARGO_BIN_EXE_crust"))
        .arg("metadata")
        .current_dir(&fixture.directory)
        .env("CC", "missing-test-compiler")
        .output()
        .unwrap();
    success(&output);
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(metadata["formatVersion"], 1);
    assert_eq!(metadata["name"], "example");
    assert_eq!(
        PathBuf::from(metadata["manifest"].as_str().unwrap()),
        fixture.directory.join("crust.toml").canonicalize().unwrap()
    );
    assert_eq!(
        PathBuf::from(metadata["directory"].as_str().unwrap()),
        fixture.directory.canonicalize().unwrap()
    );
    let targets = metadata["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0]["name"], "app");
    assert_eq!(targets[0]["kind"], "executable");
    assert_eq!(targets[0]["linkTargets"], serde_json::json!(["math"]));
    assert_eq!(targets[1]["kind"], "static-library");
    let root = fixture.directory.canonicalize().unwrap();
    let expected = [
        (
            "main.cr",
            root.join("target/crust/app/app")
                .with_extension(std::env::consts::EXE_EXTENSION),
        ),
        (
            "library.cr",
            root.join("target/crust/math").join(if cfg!(windows) {
                "math.lib"
            } else {
                "libmath.a"
            }),
        ),
    ];
    for (json, (entry, output)) in targets.iter().zip(expected) {
        assert_eq!(
            PathBuf::from(json["entry"].as_str().unwrap()),
            root.join(entry)
        );
        assert_eq!(PathBuf::from(json["output"].as_str().unwrap()), output);
    }
    assert!(!fixture.directory.join("target").exists());
}

#[test]
fn project_metadata_supports_discovery_explicit_projects_and_target_selection() {
    let fixture = Fixture::new();
    fixture.manifest("[[targets]]\nname='first'\nkind='executable'\nentry='first.cr'\n[[targets]]\nname='second'\nkind='executable'\nentry='second.cr'\n");
    fixture.write("first.cr", "int main() { return 0; }");
    fixture.write("second.cr", "int main() { return 0; }");
    fixture.write("nested/keep", "");
    for input in [
        Some(fixture.directory.join("crust.toml")),
        Some(fixture.directory.clone()),
        None,
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_crust"));
        command
            .args(["metadata", "--target", "second"])
            .current_dir(fixture.directory.join("nested"));
        if let Some(input) = input {
            command.arg("--project").arg(input);
        }
        let output = command.output().unwrap();
        success(&output);
        let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(metadata["targets"].as_array().unwrap().len(), 1);
        assert_eq!(metadata["targets"][0]["name"], "second");
    }
    let output = fixture.invoke(&["metadata", "--target", "unknown"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown target"));
    assert!(output.stdout.is_empty());
    let output = fixture.invoke(&["metadata", "first.cr"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("metadata requires"));
    assert!(!fixture.directory.join("target").exists());
}

#[test]
fn debug_metadata_and_checks_work_without_a_native_compiler() {
    let fixture = Fixture::new();
    fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\ndebug=true\n[[targets]]\nname='library'\nkind='static-library'\nentry='library.cr'\n");
    fixture.write("main.cr", "int main() { return 0; }");
    fixture.write("library.cr", "extern int answer() { return 42; }");
    for command in ["metadata", "check"] {
        let output = Command::new(env!("CARGO_BIN_EXE_crust"))
            .arg(command)
            .env("CC", "missing-compiler-for-debug-check")
            .current_dir(&fixture.directory)
            .output()
            .unwrap();
        success(&output);
        if command == "metadata" {
            let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(metadata["targets"][0]["debug"], true);
            assert_eq!(metadata["targets"][1]["debug"], false);
        }
    }
    assert!(!fixture.directory.join("target").exists());
}

#[test]
fn native_debug_information_records_original_source_files() {
    let fixture = Fixture::new();
    fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\ndebug=true\n");
    fixture.write("main.cr", "import \"math.cr\";\nint main() {\n    int value = 41;\n    print(add(value));\n    return 0;\n}\n");
    fixture.write("math.cr", "int add(int value) { return value + 1; }\n");
    let output = fixture.invoke(&["run"]);
    success(&output);
    assert_eq!(text(&output), "42\n");
    let artifact = executable(&fixture);
    if cfg!(target_os = "macos") {
        assert!(
            crust::native::debug_bundle(&artifact)
                .join("Contents/Resources/DWARF/app")
                .is_file()
        );
    } else if !cfg!(windows) {
        assert!(
            fs::read(&artifact)
                .unwrap()
                .windows(b".debug_info".len())
                .any(|bytes| bytes == b".debug_info")
        );
    }
    #[cfg(windows)]
    {
        let symbols = Command::new("llvm-pdbutil")
            .args(["dump", "-summary", "-l"])
            .arg(crust::native::debug_symbols(&artifact))
            .output()
            .unwrap();
        success(&symbols);
        let records = text(&symbols);
        assert!(
            records.contains("main.cr") && records.contains("math.cr"),
            "{records}"
        );
        let guid = records
            .lines()
            .find_map(|line| line.trim().strip_prefix("GUID: "))
            .unwrap();
        let binary = Command::new("llvm-readobj")
            .arg("--coff-debug-directory")
            .arg(&artifact)
            .output()
            .unwrap();
        success(&binary);
        assert!(text(&binary).contains(&format!("PDBGUID: {guid}")));
    }
}

#[cfg(windows)]
#[test]
fn debug_build_failures_restore_matching_binaries_import_libraries_and_pdbs() {
    let fixture = Fixture::new();
    fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\ndebug=true\n[[targets]]\nname='library'\nkind='shared-library'\nentry='library.cr'\n[targets.native]\ndebug=true\n");
    fixture.write("main.cr", "int main() { print(42); return 0; }");
    fixture.write("library.cr", "extern int answer() { return 42; }");
    success(&fixture.invoke(&["build"]));
    let app = executable(&fixture);
    let library = fixture.directory.join("target/crust/library/library.dll");
    for (target, artifact) in [("app", app.clone()), ("library", library)] {
        let pdb = crust::native::debug_symbols(&artifact);
        let mut artifacts = vec![artifact.clone(), pdb.clone()];
        if target == "library" {
            artifacts.push(crust::native::import_library(&artifact));
        }
        let previous: Vec<_> = artifacts
            .iter()
            .map(|path| fs::read(path).unwrap())
            .collect();
        assert!(previous[1].starts_with(b"Microsoft C/C++ MSF 7.00"));
        for failure in ["exit", "missing", "invalid"] {
            let compiler = compiler_stub(
                &fixture,
                failure != "exit",
                if failure == "exit" { 1 } else { 0 },
            );
            if failure != "exit" {
                let mut script = fs::read_to_string(&compiler)
                    .unwrap()
                    .replace("invalid-executable", "MZ test artifact");
                if failure == "invalid" {
                    script = script.replace(
                        "> \"%~1\" echo MZ test artifact",
                        "> \"%~1\" echo MZ test artifact\r\n> \"%~dpn1.pdb\" echo invalid-debug-symbols-padding-not-a-pdb");
                }
                fs::write(&compiler, script).unwrap();
            }
            let output = Command::new(env!("CARGO_BIN_EXE_crust"))
                .args(["build", "--target", target])
                .env("CC", compiler)
                .current_dir(&fixture.directory)
                .output()
                .unwrap();
            assert!(!output.status.success());
            if failure == "missing" {
                assert!(
                    String::from_utf8_lossy(&output.stderr)
                        .contains("did not produce debug symbols"),
                    "{target}/{failure}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            if failure == "invalid" {
                assert!(
                    String::from_utf8_lossy(&output.stderr)
                        .contains("invalid native debug symbols"),
                    "{target}/{failure}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            for (path, contents) in artifacts.iter().zip(&previous) {
                assert_eq!(
                    &fs::read(path).unwrap(),
                    contents,
                    "{failure}: {}",
                    path.display()
                );
            }
        }
    }
    let output = Command::new(&app).output().unwrap();
    success(&output);
    assert_eq!(text(&output), "42\n");
    fixture.write("main.cr", "int main() { print(7); return 0; }");
    let output = fixture.invoke(&["run", "--target", "app"]);
    success(&output);
    assert_eq!(text(&output), "7\n");
}

#[cfg(windows)]
#[test]
fn debug_symbols_cannot_overwrite_another_targets_source() {
    let fixture = Fixture::new();
    fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\ndebug=true\n[[targets]]\nname='source'\nkind='static-library'\nentry='target/crust/app/app.pdb'\n");
    fixture.write("main.cr", "int main() { return 0; }");
    let source = "extern int answer() { return 42; }";
    fixture.write("target/crust/app/app.pdb", source);
    let output = fixture.invoke(&["build", "--target", "app"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("overwrite an input"));
    assert_eq!(
        fs::read_to_string(fixture.directory.join("target/crust/app/app.pdb")).unwrap(),
        source
    );
    assert!(!executable(&fixture).exists());
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
fn native_macro_definitions_preserve_values_function_macros_and_per_target_configuration() {
    let fixture = Fixture::new();
    fixture.write(
        "main.cr",
        "extern int native_value(); int main() { unsafe { print(native_value()); } return 0; }",
    );
    fixture.write("native.c", "#include <string.h>\nint native_value(void) { return strcmp(API_TEXT, \"hello world\") == 0 ? API_VALUE + NATIVE_DOUBLE(1) : -1; }\n");
    fixture.manifest(
        r#"
        [[targets]]
        name = 'first'
        kind = 'executable'
        entry = 'main.cr'
        [targets.native]
        sources = ['native.c']
        defines = ['API_VALUE=40', 'NATIVE_DOUBLE(x)=((x)*2)', 'API_TEXT="hello world"']
        [[targets]]
        name = 'second'
        kind = 'executable'
        entry = 'main.cr'
        [targets.native]
        sources = ['native.c']
        defines = ['API_VALUE=41', 'NATIVE_DOUBLE(x)=((x)*2)', 'API_TEXT="hello world"']
    "#,
    );
    for (target, expected) in [("first", "42\n"), ("second", "43\n")] {
        let output = fixture.invoke(&["run", "--target", target]);
        success(&output);
        assert_eq!(text(&output), expected);
    }
}

#[test]
fn native_target_link_planning_handles_long_chains_without_recursive_traversal() {
    let fixture = Fixture::new();
    fixture.write("library.cr", "extern int answer() { return 42; }");
    let mut targets = String::new();
    for index in 0..1200 {
        targets.push_str(&format!(
            "[[targets]]\nname='node_{index}'\nkind='static-library'\nentry='library.cr'\n"
        ));
        if index + 1 < 1200 {
            targets.push_str(&format!(
                "[targets.native]\nlink-targets=['node_{}']\n",
                index + 1
            ));
        }
    }
    fixture.manifest(&targets);
    let project = crust::project::Project::load(&fixture.directory).unwrap();
    let root = project.select(Some("node_0"), true).unwrap()[0];
    let order = project.ordered_targets(&[root]).unwrap();
    assert_eq!(order.len(), 1200);
    assert_eq!(order.first().unwrap().name, "node_1199");
    assert_eq!(order.last().unwrap().name, "node_0");
}

#[test]
fn native_drivers_require_project_target_links_to_be_resolved_before_compilation() {
    let options = crust::native::Options {
        link_targets: vec!["library".to_owned()],
        ..Default::default()
    };
    for result in [
        crust::native::compile_file_with_options(
            Path::new("missing.c"),
            Path::new("unused.exe"),
            &options,
        ),
        crust::native::compile_static_library(
            Path::new("missing.c"),
            Path::new("unused.a"),
            &options,
        ),
        crust::native::compile_shared_library(
            Path::new("missing.c"),
            Path::new("unused.dll"),
            &options,
        ),
    ] {
        assert!(result.unwrap_err().contains("resolved by a project build"));
    }
}

#[test]
fn native_target_links_build_dependencies_before_a_selected_executable() {
    let fixture = Fixture::new();
    fixture.box_library();
    fixture.write(
        "crust.toml",
        include_str!("../examples/linked_project/crust.toml"),
    );
    fixture.write(
        "src/main.cr",
        include_str!("../examples/linked_project/src/main.cr"),
    );
    fixture.write(
        "src/math.cr",
        &include_str!("../examples/linked_project/src/math.cr")
            .replace("../../../lib/box.cr", "../lib/box.cr"),
    );
    let output = fixture.invoke(&["run", "--target", "app"]);
    success(&output);
    assert_eq!(text(&output), "42\n");
    let archive = fixture
        .directory
        .join("target/crust/math")
        .join(if cfg!(windows) {
            "math.lib"
        } else {
            "libmath.a"
        });
    assert!(archive.is_file());
    fixture.write(
        "src/math.cr",
        "extern int add(int left, int right) = \"linked_add\" { return missing; }",
    );
    let output = fixture.invoke(&["check", "--target", "app"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing"));
}

#[test]
fn external_c_runtime_libraries_flow_through_static_dependencies_and_metadata() {
    let fixture = Fixture::new();
    fixture.write("vendor, data; files/vendor.c", "#if defined(_WIN32)\n__declspec(dllexport)\n#endif\nint vendor_answer(void) { return 42; }\n");
    let base = fixture.directory.join("vendor, data; files/vendor");
    let library = crust::project::TargetKind::SharedLibrary
        .artifact_path(&base)
        .unwrap();
    crust::native::compile_shared_library(&base.with_extension("c"), &library, &Default::default())
        .unwrap();
    let relative = library
        .strip_prefix(&fixture.directory)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    fixture.manifest(&format!("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nlink-targets=['bridge']\n[[targets]]\nname='bridge'\nkind='static-library'\nentry='bridge.cr'\n[targets.native]\nlibrary-dirs=['vendor, data; files']\nlibraries=['vendor']\nruntime-libraries=['{relative}']\n"));
    fixture.write(
        "main.cr",
        "extern int bridge(); int main() { unsafe { print(bridge()); } return 0; }",
    );
    fixture.write(
        "bridge.cr",
        "extern int vendor_answer(); extern int bridge() { return vendor_answer(); }",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_crust"))
        .arg("metadata")
        .env("CC", "missing-runtime-test-compiler")
        .current_dir(&fixture.directory)
        .output()
        .unwrap();
    success(&output);
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    for target in metadata["targets"].as_array().unwrap() {
        assert_eq!(target["runtimeLibraries"].as_array().unwrap().len(), 1);
        assert_eq!(
            PathBuf::from(target["runtimeLibraries"][0].as_str().unwrap()),
            library.canonicalize().unwrap()
        );
        assert_eq!(target["runtimePaths"].as_array().unwrap().len(), 1);
    }
    let output = fixture.invoke(&["run", "--target", "app"]);
    success(&output);
    assert_eq!(text(&output), "42\n");
    let output = Command::new(executable(&fixture)).output().unwrap();
    success(&output);
    assert_eq!(text(&output), "42\n");
    if cfg!(windows) {
        assert!(
            executable(&fixture)
                .parent()
                .unwrap()
                .join(library.file_name().unwrap())
                .is_file()
        );
    }
}

#[test]
fn runtime_library_configuration_validates_paths_and_allows_linking_the_same_file() {
    let fixture = Fixture::new();
    fixture.write("main.cr", "int main() { return 0; }");
    fixture.write("vendor/library.bin", "native input placeholder");
    for (paths, message) in [
        ("['missing.dll']", "runtime library"),
        ("['vendor']", "must be a file"),
        (
            "['vendor/library.bin', 'vendor/./library.bin']",
            "repeats native input",
        ),
    ] {
        fixture.manifest(&format!("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nruntime-libraries={paths}\n"));
        let output = fixture.invoke(&["metadata"]);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(message),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nlink-inputs=['vendor/library.bin']\nruntime-libraries=['vendor/library.bin']\n");
    success(&fixture.invoke(&["check"]));
    assert!(!fixture.directory.join("target").exists());
}

#[test]
fn external_runtime_libraries_are_protected_from_other_targets_outputs() {
    let fixture = Fixture::new();
    fixture.write("main.cr", "int main() { return 0; }");
    fixture.write("library.cr", "extern int answer() { return 42; }");
    let project_base = fixture.directory.join("target/crust/producer/producer");
    let library = crust::project::TargetKind::SharedLibrary
        .artifact_path(&project_base)
        .unwrap();
    let relative = library
        .strip_prefix(&fixture.directory)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    fixture.write(&relative, "external library input");
    fixture.manifest(&format!("[[targets]]\nname='producer'\nkind='shared-library'\nentry='library.cr'\n[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nruntime-libraries=['{relative}']\n"));
    let output = fixture.invoke(&["build", "--target", "producer"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("output would overwrite an input file")
    );
    assert_eq!(
        fs::read_to_string(library).unwrap(),
        "external library input"
    );
    assert!(!project_base.with_extension("c").exists());
}

#[test]
fn shared_library_targets_link_through_static_dependencies_and_report_runtime_paths() {
    let fixture = Fixture::new();
    fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nlink-targets=['bridge', 'widget']\n[[targets]]\nname='bridge'\nkind='static-library'\nentry='bridge.cr'\n[targets.native]\nlink-targets=['widget']\n[[targets]]\nname='widget'\nkind='shared-library'\nentry='widget.cr'\n[targets.native]\nlink-targets=['base']\n[[targets]]\nname='base'\nkind='static-library'\nentry='base.cr'\n");
    fixture.write("main.cr", "extern int bridge(); extern int shared_add(int); int main() { unsafe { print(bridge()); print(shared_add(2)); } return 0; }");
    fixture.write(
        "bridge.cr",
        "extern int shared_add(int); extern int bridge() { return shared_add(1); }",
    );
    fixture.write("widget.cr", "extern int base_value(); int helper(int value) { return value; } extern int add(int input) = \"shared_add\" { Box<int> value = Box<int>::create(input); return base_value() + helper(value.get()); }");
    fixture.write("base.cr", "extern int base_value() { return 40; }");
    let output = Command::new(env!("CARGO_BIN_EXE_crust"))
        .args(["check", "--target", "app"])
        .env("CC", "missing-shared-test-compiler")
        .current_dir(&fixture.directory)
        .output()
        .unwrap();
    success(&output);
    assert!(!fixture.directory.join("target").exists());
    let metadata = fixture.invoke(&["metadata"]);
    success(&metadata);
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();
    assert_eq!(metadata["targets"][2]["kind"], "shared-library");
    assert_eq!(
        metadata["targets"][0]["runtimePaths"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        metadata["targets"][2]["runtimePaths"],
        serde_json::json!([])
    );
    let output = fixture.invoke(&["run", "--target", "app"]);
    success(&output);
    assert_eq!(text(&output), "41\n42\n");
    let project = crust::project::Project::load(&fixture.directory).unwrap();
    let library = project.select(Some("widget"), true).unwrap()[0];
    let artifact = library
        .kind
        .artifact_path(&project.output_base(library))
        .unwrap();
    assert!(artifact.is_file());
    if cfg!(windows) {
        assert!(crust::native::import_library(&artifact).is_file());
    }
    let header = fixture.invoke(&["emit-header", "--target", "widget"]);
    success(&header);
    assert!(text(&header).contains("shared_add"));
    assert!(!text(&header).contains("helper"));
    let run = fixture.invoke(&["run", "--target", "widget"]);
    assert!(!run.status.success());
    assert!(String::from_utf8_lossy(&run.stderr).contains("shared libraries are built"));
}

#[test]
fn shared_library_rebuild_failures_preserve_native_and_import_artifacts() {
    let fixture = Fixture::new();
    fixture.manifest("[[targets]]\nname='library'\nkind='shared-library'\nentry='library.cr'\n");
    fixture.write("library.cr", "extern int answer() { return 42; }");
    success(&fixture.invoke(&["build"]));
    let project = crust::project::Project::load(&fixture.directory).unwrap();
    let target = &project.targets[0];
    let artifact = target
        .kind
        .artifact_path(&project.output_base(target))
        .unwrap();
    let mut artifacts = vec![artifact.clone()];
    if cfg!(windows) {
        artifacts.push(crust::native::import_library(&artifact));
    }
    let previous: Vec<_> = artifacts
        .iter()
        .map(|path| fs::read(path).unwrap())
        .collect();
    for (writes, code) in [(false, 0), (true, 0), (true, 1)] {
        let compiler = compiler_stub(&fixture, writes, code);
        let output = Command::new(env!("CARGO_BIN_EXE_crust"))
            .arg("build")
            .env("CC", compiler)
            .current_dir(&fixture.directory)
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "a compiler stub must not reuse old shared outputs"
        );
        for (path, contents) in artifacts.iter().zip(&previous) {
            assert_eq!(&fs::read(path).unwrap(), contents);
        }
    }
    #[cfg(windows)]
    {
        // A native-looking DLL still needs a fresh import library.
        let compiler = compiler_stub(&fixture, true, 0);
        let script = fs::read_to_string(&compiler)
            .unwrap()
            .replace("invalid-executable", "MZ DLL");
        fs::write(&compiler, script).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_crust"))
            .arg("build")
            .env("CC", compiler)
            .current_dir(&fixture.directory)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("did not produce an import library")
        );
        for (path, contents) in artifacts.iter().zip(&previous) {
            assert_eq!(&fs::read(path).unwrap(), contents);
        }
    }
    fixture.write("library.cr", "extern int answer() { return 7; }");
    success(&fixture.invoke(&["build"]));
    assert_ne!(fs::read(&artifact).unwrap(), previous[0]);
    assert!(
        fs::read_dir(artifact.parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("crust-build-"))
    );
}

#[test]
#[cfg(windows)]
fn failed_executable_links_restore_deployed_dlls_and_application_manifests() {
    let fixture = Fixture::new();
    fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nlink-targets=['library']\n[[targets]]\nname='library'\nkind='shared-library'\nentry='library.cr'\n");
    let main = "extern int answer(); int main() { unsafe { print(answer()); } return 0; }";
    fixture.write("main.cr", main);
    fixture.write("library.cr", "extern int answer() { return 42; }");
    success(&fixture.invoke(&["build", "--target", "app"]));
    let program = executable(&fixture);
    let deployed = program.parent().unwrap().join("library.dll");
    let manifest = crust::native::application_manifest(&program);
    let paths = [&program, &deployed, &manifest];
    let previous: Vec<_> = paths.iter().map(|path| fs::read(path).unwrap()).collect();
    fixture.write("library.cr", "extern int answer() { return 7; }");
    fixture.write("main.cr", "extern int answer(); extern int missing(); int main() { unsafe { print(answer()); return missing(); } }");
    let output = fixture.invoke(&["build", "--target", "app"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("C compiler failed"));
    for (path, contents) in paths.iter().zip(previous) {
        assert_eq!(fs::read(path).unwrap(), contents);
    }
    let output = Command::new(&program).output().unwrap();
    success(&output);
    assert_eq!(text(&output), "42\n");
    fixture.write("main.cr", main);
    let output = fixture.invoke(&["run", "--target", "app"]);
    success(&output);
    assert_eq!(text(&output), "7\n");
    assert_no_build_staging(&fixture);
}

#[test]
#[cfg(windows)]
fn shared_library_import_outputs_preserve_native_inputs() {
    let fixture = Fixture::new();
    fixture.write("library.cr", "extern int answer() { return 42; }");
    fixture.write(
        "target/crust/library/library.lib",
        "int native_source(void) { return 7; }",
    );
    fixture.manifest("[[targets]]\nname='library'\nkind='shared-library'\nentry='library.cr'\n[targets.native]\nsources=['target/crust/library/library.lib']\n");
    let output = fixture.invoke(&["build"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("output would overwrite an input file")
    );
    assert_eq!(
        fs::read_to_string(fixture.directory.join("target/crust/library/library.lib")).unwrap(),
        "int native_source(void) { return 7; }"
    );
    assert!(
        !fixture
            .directory
            .join("target/crust/library/library.c")
            .exists()
    );
}

#[test]
#[cfg(windows)]
fn deployed_dlls_and_application_manifests_preserve_project_inputs() {
    for source in ["library.dll", "app.exe.manifest"] {
        let fixture = Fixture::new();
        fixture.write("library.cr", "extern int answer() { return 42; }");
        fixture.write("main.cr", "int main() { return 0; }");
        let input = format!("target/crust/app/{source}");
        fixture.write(&input, "int native_source(void) { return 7; }");
        fixture.manifest(&format!("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nlink-targets=['library']\nsources=['{input}']\n[[targets]]\nname='library'\nkind='shared-library'\nentry='library.cr'\n"));
        let output = fixture.invoke(&["build", "--target", "app"]);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("output would overwrite an input file")
        );
        assert_eq!(
            fs::read_to_string(fixture.directory.join(input)).unwrap(),
            "int native_source(void) { return 7; }"
        );
        assert!(!fixture.directory.join("target/crust/app/app.c").exists());
    }
}

#[test]
fn shared_library_chains_run_and_link_from_a_c_consumer() {
    let mut fixture = Fixture::new();
    fixture.directory.push("paths, with; delimiters");
    fs::create_dir_all(&fixture.directory).unwrap();
    fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nlink-targets=['outer']\n[[targets]]\nname='outer'\nkind='shared-library'\nentry='outer.cr'\n[targets.native]\nlink-targets=['inner']\n[[targets]]\nname='inner'\nkind='shared-library'\nentry='inner.cr'\n");
    fixture.write(
        "main.cr",
        "extern int answer(); int main() { unsafe { print(answer()); } return 0; }",
    );
    fixture.write(
        "outer.cr",
        "extern int inner_answer(); extern int answer() { return inner_answer() + 1; }",
    );
    fixture.write("inner.cr", "extern int inner_answer() { return 41; }");
    let output = fixture.invoke(&["run", "--target", "app"]);
    success(&output);
    assert_eq!(text(&output), "42\n");
    let project = crust::project::Project::load(&fixture.directory).unwrap();
    let app = project.select(Some("app"), true).unwrap()[0];
    let options = project.resolved_native(app).unwrap();
    assert_eq!(options.runtime_dirs.len(), 2);
    fixture.write("consumer.c", "#include <stdio.h>\nextern int answer(void); int main(void) { printf(\"%d\\n\", answer()); return 0; }\n");
    let executable = fixture
        .directory
        .join("consumer")
        .with_extension(std::env::consts::EXE_EXTENSION);
    crust::native::compile_file_with_options(
        &fixture.directory.join("consumer.c"),
        &executable,
        &options,
    )
    .unwrap();
    let variable = if cfg!(windows) {
        "PATH"
    } else if cfg!(target_os = "macos") {
        "DYLD_LIBRARY_PATH"
    } else {
        "LD_LIBRARY_PATH"
    };
    let mut paths = options.runtime_dirs;
    paths.extend(std::env::split_paths(
        &std::env::var_os(variable).unwrap_or_default(),
    ));
    let output = Command::new(executable)
        .env(variable, std::env::join_paths(paths).unwrap())
        .output()
        .unwrap();
    success(&output);
    assert_eq!(text(&output), "42\n");
}

#[test]
fn shared_and_static_libraries_can_build_the_same_source_entry() {
    let fixture = Fixture::new();
    fixture.manifest("[[targets]]\nname='dynamic'\nkind='shared-library'\nentry='library.cr'\n[[targets]]\nname='archive'\nkind='static-library'\nentry='library.cr'\n");
    fixture.write(
        "library.cr",
        "int helper() { return 42; } extern int answer() { return helper(); }",
    );
    success(&fixture.invoke(&["build"]));
    let project = crust::project::Project::load(&fixture.directory).unwrap();
    for target in &project.targets {
        assert!(
            target
                .kind
                .artifact_path(&project.output_base(target))
                .unwrap()
                .is_file()
        );
    }
}

#[test]
fn native_target_link_graphs_reject_unknown_repeated_executable_and_cyclic_dependencies() {
    let fixture = Fixture::new();
    fixture.write("library.cr", "extern int value() { return 0; }");
    fixture.write("main.cr", "int main() { return 0; }");
    for (targets, message) in [
        (
            "[[targets]]\nname='a'\nkind='static-library'\nentry='library.cr'\n[targets.native]\nlink-targets=['missing']\n",
            "unknown target",
        ),
        (
            "[[targets]]\nname='a'\nkind='static-library'\nentry='library.cr'\n[targets.native]\nlink-targets=['a']\n",
            "a -> a",
        ),
        (
            "[[targets]]\nname='a'\nkind='static-library'\nentry='library.cr'\n[targets.native]\nlink-targets=['b']\n[[targets]]\nname='b'\nkind='static-library'\nentry='library.cr'\n[targets.native]\nlink-targets=['a']\n",
            "a -> b -> a",
        ),
        (
            "[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nlink-targets=['library', 'library']\n[[targets]]\nname='library'\nkind='static-library'\nentry='library.cr'\n",
            "repeats linked target",
        ),
        (
            "[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nlink-targets=['app']\n",
            "must be a static-library or shared-library target",
        ),
    ] {
        fixture.manifest(targets);
        let output = fixture.invoke(&["check"]);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(message),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn native_target_link_order_deduplicates_shared_dependencies_and_keeps_them_after_consumers() {
    let fixture = Fixture::new();
    fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nlink-targets=['left', 'right']\n[[targets]]\nname='left'\nkind='static-library'\nentry='left.cr'\n[targets.native]\nlink-targets=['base']\n[[targets]]\nname='right'\nkind='static-library'\nentry='right.cr'\n[targets.native]\nlink-targets=['base']\n[[targets]]\nname='base'\nkind='static-library'\nentry='base.cr'\n[targets.native]\nsources=['first.c', 'second.c']\n");
    fixture.write("main.cr", "extern int left(); extern int right(); int main() { unsafe { print(left() + right()); } return 0; }");
    fixture.write(
        "left.cr",
        "extern int base_first(); extern int left() { return base_first(); }",
    );
    fixture.write(
        "right.cr",
        "extern int base_second(); extern int right() { return base_second(); }",
    );
    fixture.write(
        "base.cr",
        "extern int base_first(); extern int base_second();",
    );
    fixture.write("first.c", "int base_first(void) { return 20; }");
    fixture.write("second.c", "int base_second(void) { return 22; }");
    let project = crust::project::Project::load(&fixture.directory).unwrap();
    let app = project.select(Some("app"), true).unwrap()[0];
    let order = project.ordered_targets(&[app]).unwrap();
    assert_eq!(
        order.iter().filter(|target| target.name == "base").count(),
        1
    );
    assert_eq!(order.first().unwrap().name, "base");
    assert_eq!(order.last().unwrap().name, "app");
    let inputs = project.resolved_native(app).unwrap().link_inputs;
    assert_eq!(inputs.len(), 3);
    assert_eq!(
        inputs.last().unwrap().file_name().unwrap(),
        if cfg!(windows) {
            "base.lib"
        } else {
            "libbase.a"
        }
    );
    let output = fixture.invoke(&["run", "--target", "app"]);
    success(&output);
    assert_eq!(text(&output), "42\n");
}

#[test]
fn emit_header_uses_library_target_settings_without_invoking_a_c_compiler() {
    let fixture = Fixture::new();
    fixture.manifest("[[targets]]\nname='library'\nkind='static-library'\nentry='library.cr'\n");
    fixture.write("library.cr", "import \"helper.cr\"; extern int add(int left, int right) = \"native_add\" { return sum(left, right); }");
    fixture.write(
        "helper.cr",
        "int sum(int left, int right) { return left + right; }",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_crust"))
        .arg("emit-header")
        .current_dir(&fixture.directory)
        .env("CC", "missing-crust-test-compiler")
        .output()
        .unwrap();
    success(&output);
    let header = text(&output);
    assert!(header.starts_with("#pragma once"));
    fixture.write("public.h", &header);
    success(&fixture.invoke(&["build"]));
    fixture.write("consumer.c", "#include \"public.h\"\n#include <stdio.h>\nint main(void) { printf(\"%d\\n\", (int)native_add(20, 22)); return 0; }\n");
    let archive = fixture
        .directory
        .join("target/crust/library")
        .join(if cfg!(windows) {
            "library.lib"
        } else {
            "liblibrary.a"
        });
    let consumer = fixture
        .directory
        .join("consumer")
        .with_extension(std::env::consts::EXE_EXTENSION);
    crust::native::compile_file_with_options(
        &fixture.directory.join("consumer.c"),
        &consumer,
        &crust::native::Options {
            link_inputs: vec![archive],
            ..Default::default()
        },
    )
    .unwrap();
    let output = Command::new(consumer).output().unwrap();
    success(&output);
    assert_eq!(text(&output), "42\n");
}

#[test]
fn static_libraries_keep_explicit_objects_with_the_same_basename() {
    let fixture = Fixture::new();
    fixture.write("one/input.c", "int native_first(void) { return 20; }");
    fixture.write("two/input.c", "int native_second(void) { return 22; }");
    let compiler = std::env::var_os("CC").unwrap_or_else(|| {
        if cfg!(windows) {
            "clang".into()
        } else {
            "cc".into()
        }
    });
    for directory in ["one", "two"] {
        let output = Command::new(&compiler)
            .args(["-std=c11", "-c"])
            .arg(fixture.directory.join(directory).join("input.c"))
            .arg("-o")
            .arg(fixture.directory.join(directory).join("input.o"))
            .output()
            .unwrap();
        success(&output);
    }
    fixture.manifest("[[targets]]\nname='objects'\nkind='static-library'\nentry='library.cr'\n[targets.native]\nlink-inputs=['one/input.o', 'two/input.o']\n");
    fixture.write("library.cr", "extern int native_first(); extern int native_second(); extern int answer() { return native_first() + native_second(); }");
    success(&fixture.invoke(&["build"]));
    let artifact = fixture
        .directory
        .join("target/crust/objects")
        .join(if cfg!(windows) {
            "objects.lib"
        } else {
            "libobjects.a"
        });
    fixture.write("consumer.c", "#include <stdint.h>\n#include <stdio.h>\nint32_t answer(void); int main(void) { printf(\"%d\\n\", (int)answer()); return 0; }\n");
    let consumer = fixture
        .directory
        .join("consumer")
        .with_extension(std::env::consts::EXE_EXTENSION);
    crust::native::compile_file_with_options(
        &fixture.directory.join("consumer.c"),
        &consumer,
        &crust::native::Options {
            link_inputs: vec![artifact],
            ..Default::default()
        },
    )
    .unwrap();
    let output = Command::new(consumer).output().unwrap();
    success(&output);
    assert_eq!(text(&output), "42\n");
}

#[test]
fn library_rebuilds_drop_removed_members_and_helpers_do_not_collide_between_archives() {
    let fixture = Fixture::new();
    fixture.manifest("[[targets]]\nname='first'\nkind='static-library'\nentry='first.cr'\n[targets.native]\nsources=['removed.c']\n[[targets]]\nname='second'\nkind='static-library'\nentry='second.cr'\n");
    fixture.write("first.cr", "int helper(int value) { return value + 1; } extern int first(int value) { return helper(value); }");
    fixture.write("second.cr", "int helper(int value) { return value * 2; } extern int second(int value) { return helper(value); }");
    fixture.write("removed.c", "int removed_symbol(void) { return 99; }");
    success(&fixture.invoke(&["build"]));
    let artifact = |name: &str| {
        fixture
            .directory
            .join("target/crust")
            .join(name)
            .join(if cfg!(windows) {
                format!("{name}.lib")
            } else {
                format!("lib{name}.a")
            })
    };
    let archiver = std::env::var_os("AR").unwrap_or_else(|| {
        if cfg!(windows) {
            "llvm-ar".into()
        } else {
            "ar".into()
        }
    });
    let members = Command::new(&archiver)
        .arg("t")
        .arg(artifact("first"))
        .output()
        .unwrap();
    success(&members);
    assert_eq!(
        text(&members)
            .lines()
            .filter(|name| name.ends_with(".o"))
            .count(),
        2
    );
    fixture.manifest("[[targets]]\nname='first'\nkind='static-library'\nentry='first.cr'\n[[targets]]\nname='second'\nkind='static-library'\nentry='second.cr'\n");
    success(&fixture.invoke(&["build"]));
    let members = Command::new(&archiver)
        .arg("t")
        .arg(artifact("first"))
        .output()
        .unwrap();
    success(&members);
    assert_eq!(
        text(&members)
            .lines()
            .filter(|name| name.ends_with(".o"))
            .count(),
        1
    );
    fixture.write("consumer.c", "#include <stdint.h>\n#include <stdio.h>\nint32_t first(int32_t); int32_t second(int32_t); int main(void) { printf(\"%d\\n\", (int)(first(20) + second(10))); return 0; }\n");
    let consumer = fixture
        .directory
        .join("consumer")
        .with_extension(std::env::consts::EXE_EXTENSION);
    crust::native::compile_file_with_options(
        &fixture.directory.join("consumer.c"),
        &consumer,
        &crust::native::Options {
            link_inputs: vec![artifact("first"), artifact("second")],
            ..Default::default()
        },
    )
    .unwrap();
    let output = Command::new(consumer).output().unwrap();
    success(&output);
    assert_eq!(text(&output), "41\n");
}

#[test]
fn static_library_configuration_and_outputs_preserve_inputs() {
    let fixture = Fixture::new();
    fixture.write("library.cr", "extern int answer() { return 42; }");
    fixture.write("other.a", "preserved");
    fixture.manifest("[[targets]]\nname='app'\nkind='static-library'\nentry='library.cr'\n[targets.native]\nlink-inputs=['other.a']\n");
    let output = fixture.invoke(&["check"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("static-library"));
    fixture.manifest("[[targets]]\nname='library'\nkind='static-library'\nentry='library.cr'\n[[targets]]\nname='app'\nkind='executable'\nentry='library.cr'\n");
    let output = fixture.invoke(&["check"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("incompatible target kinds"));
    let filename = if cfg!(windows) {
        "library.lib"
    } else {
        "liblibrary.a"
    };
    let entry = format!("target/crust/library/{filename}");
    let source = "extern int answer() { return 42; }";
    fixture.write(&entry, source);
    fixture.manifest(&format!(
        "[[targets]]\nname='library'\nkind='static-library'\nentry='{entry}'\n"
    ));
    let output = fixture.invoke(&["build"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("overwrite an input"));
    assert_eq!(
        fs::read_to_string(fixture.directory.join(entry)).unwrap(),
        source
    );
}

#[test]
fn static_library_targets_archive_native_sources_and_link_from_a_c_consumer() {
    let fixture = Fixture::new();
    fixture.box_library();
    fixture.write(
        "crust.toml",
        include_str!("../examples/library_project/crust.toml"),
    );
    fixture.write(
        "src/library.cr",
        &include_str!("../examples/library_project/src/library.cr")
            .replace("../../../lib/box.cr", "../lib/box.cr"),
    );
    fixture.write(
        "include/math.h",
        include_str!("../examples/library_project/include/math.h"),
    );
    fixture.write(
        "native/increment.c",
        include_str!("../examples/library_project/native/increment.c"),
    );
    fixture.write(
        "native/consumer.c",
        include_str!("../examples/library_project/native/consumer.c"),
    );
    let output = fixture.invoke(&["check"]);
    success(&output);
    let output = fixture.invoke(&["build"]);
    success(&output);
    let artifact = fixture
        .directory
        .join("target/crust/math")
        .join(if cfg!(windows) {
            "math.lib"
        } else {
            "libmath.a"
        });
    assert!(artifact.is_file(), "{}", text(&output));
    let consumer = fixture
        .directory
        .join("consumer")
        .with_extension(std::env::consts::EXE_EXTENSION);
    crust::native::compile_file_with_options(
        &fixture.directory.join("native/consumer.c"),
        &consumer,
        &crust::native::Options {
            include_dirs: vec![fixture.directory.join("include")],
            link_inputs: vec![artifact.clone()],
            ..Default::default()
        },
    )
    .unwrap();
    let output = Command::new(consumer).output().unwrap();
    success(&output);
    assert_eq!(text(&output), "42\n");
    let previous = fs::read(&artifact).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_crust"))
        .arg("build")
        .current_dir(&fixture.directory)
        .env("AR", "missing-crust-test-archiver")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("could not start archiver"));
    assert_eq!(fs::read(&artifact).unwrap(), previous);
    for invalid in [false, true] {
        let name = if cfg!(windows) {
            "archiver.cmd"
        } else {
            "archiver.sh"
        };
        let script = if cfg!(windows) {
            if invalid {
                "@echo invalid archive>\"%~2\"\r\n@exit /b 0\r\n"
            } else {
                "@exit /b 0\r\n"
            }
        } else if invalid {
            "#!/bin/sh\nprintf 'invalid archive' > \"$2\"\nexit 0\n"
        } else {
            "#!/bin/sh\nexit 0\n"
        };
        fixture.write(name, script);
        let script_path = fixture.directory.join(name);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let output = Command::new(env!("CARGO_BIN_EXE_crust"))
            .arg("build")
            .current_dir(&fixture.directory)
            .env("AR", script_path)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("regular archive"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read(&artifact).unwrap(), previous);
    }
    assert!(
        fs::read_dir(artifact.parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("crust-build-"))
    );
    let output = fixture.invoke(&["run"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("static libraries"));
}

#[test]
fn separate_native_sources_call_named_crust_exports() {
    let fixture = Fixture::new();
    fixture.box_library();
    fixture.write(
        "crust.toml",
        include_str!("../examples/export_project/crust.toml"),
    );
    fixture.write(
        "src/main.cr",
        &include_str!("../examples/export_project/src/main.cr")
            .replace("../../../lib/box.cr", "../lib/box.cr"),
    );
    fixture.write(
        "include/exports.h",
        include_str!("../examples/export_project/include/exports.h"),
    );
    fixture.write(
        "native/caller.c",
        include_str!("../examples/export_project/native/caller.c"),
    );
    let output = fixture.invoke(&["run"]);
    success(&output);
    assert_eq!(text(&output), "42\n");
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
        "defines = ['']",
        "defines = ['1VALUE=1']",
        "defines = ['VALUE=1', 'VALUE=2']",
        "defines = ['FUNCTION(x=2']",
        "defines = 'VALUE=1'",
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
fn project_builds_protect_native_headers_referenced_by_any_target() {
    let fixture = Fixture::new();
    fixture.write("first.cr", "int main() { return 0; }");
    fixture.write(
        "second.cr",
        "extern header \"target/crust/first/first.c\"; int main() { return 0; }",
    );
    fixture.write("target/crust/first/first.c", "/* native header input */\n");
    fixture.manifest("[[targets]]\nname='first'\nkind='executable'\nentry='first.cr'\n[[targets]]\nname='second'\nkind='executable'\nentry='second.cr'\n");
    let output = fixture.invoke(&["build", "--target", "first"]);
    assert!(!output.status.success());
    assert_eq!(
        std::fs::read_to_string(fixture.directory.join("target/crust/first/first.c")).unwrap(),
        "/* native header input */\n"
    );
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
fn native_integer_types_survive_variadic_calls_and_repeated_imported_declarations() {
    let fixture = Fixture::new();
    fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nsources=['native.c']\n");
    fixture.write("native.c", r#"
        #include <stdarg.h>
        #include <stddef.h>
        long read_long(int ignored, ...) { va_list args; va_start(args, ignored); long result = va_arg(args, long); va_end(args); return result; }
        unsigned long read_ulong(int ignored, ...) { va_list args; va_start(args, ignored); unsigned long result = va_arg(args, unsigned long); va_end(args); return result; }
        long long read_wide(int ignored, ...) { va_list args; va_start(args, ignored); long long result = va_arg(args, long long); va_end(args); return result; }
        unsigned long long read_uwide(int ignored, ...) { va_list args; va_start(args, ignored); unsigned long long result = va_arg(args, unsigned long long); va_end(args); return result; }
        size_t native_long_size(void) { return sizeof(long); }
    "#);
    fixture.write(
        "a.cr",
        "typedef long Native; extern long read_long(int, ...);",
    );
    fixture.write(
        "b.cr",
        "typedef long Native; extern Native read_long(int ignored, ...);",
    );
    fixture.write("main.cr", r#"
        import "a.cr"; import "b.cr";
        extern unsigned long read_ulong(int ignored, ...);
        extern long long read_wide(int ignored, ...);
        extern unsigned long long read_uwide(int ignored, ...);
        extern size_t native_long_size();
        int main() {
            Native value = -2147483647; unsigned long bits = ~(unsigned long)0;
            long long wide = -9223372036854775807; unsigned long long largest = 18446744073709551615;
            unsafe {
                print(read_long(0, value)); print(read_ulong(0, bits) == bits);
                print(read_wide(0, wide)); print(read_uwide(0, largest));
                print(sizeof(long) == native_long_size());
                print(read_ulong(0, (int)1 + (unsigned long)2));
                print(read_ulong(0, (unsigned long)((unsigned)1 + (long)2)));
            }
            return 0;
        }
    "#);
    let output = fixture.invoke(&["run"]);
    success(&output);
    assert_eq!(
        text(&output),
        "-2147483647\n1\n-9223372036854775807\n18446744073709551615\n1\n3\n3\n"
    );
}

#[cfg(windows)]
#[test]
fn native_libraries_and_relative_driver_inputs_build_in_percent_named_directories() {
    let parent = Fixture::new();
    let fixture = Fixture {
        directory: parent.directory.join("native % path; and & symbols"),
    };
    fs::create_dir(&fixture.directory).unwrap();
    fixture.manifest(
        r#"
        [[targets]]
        name = 'core'
        kind = 'static-library'
        entry = 'core.cr'
        [targets.native]
        sources = ['helper.c']
        include-dirs = ['include']
        debug = true
        [[targets]]
        name = 'shared'
        kind = 'shared-library'
        entry = 'shared.cr'
        [targets.native]
        link-targets = ['core']
        debug = true
        [[targets]]
        name = 'app'
        kind = 'executable'
        entry = 'main.cr'
        [targets.native]
        link-targets = ['shared']
        debug = true
    "#,
    );
    fixture.write("include/offset.h", "#define OFFSET 1\n");
    fixture.write(
        "helper.c",
        "#include \"offset.h\"\nint helper(int value) { return value + OFFSET; }\n",
    );
    fixture.write("core.cr", "extern int helper(int); extern int core_answer(int value) { unsafe { return helper(value); } }");
    fixture.write("shared.cr", "extern int core_answer(int); extern int answer(int value) { unsafe { return core_answer(value); } }");
    fixture.write(
        "main.cr",
        "extern int answer(int); int main() { unsafe { print(answer(41)); } return 0; }",
    );
    let output = fixture.invoke(&["run", "--target", "app"]);
    success(&output);
    assert_eq!(text(&output), "42\n");
    for (name, file) in [("app", "app.exe"), ("shared", "shared.dll")] {
        let pdb = crust::native::debug_symbols(
            &fixture.directory.join("target/crust").join(name).join(file),
        );
        assert!(
            fs::read(pdb)
                .unwrap()
                .starts_with(b"Microsoft C/C++ MSF 7.00")
        );
    }
    assert!(
        !fixture
            .directory
            .join("target/crust/core/core.pdb")
            .exists()
    );

    // Public driver paths retain their meaning relative to the caller even
    // when the native tool runs from the output directory.
    let relative = fixture
        .directory
        .strip_prefix(env!("CARGO_MANIFEST_DIR"))
        .unwrap();
    fixture.write("relative.c", "#include \"offset.h\"\nextern int answer(int); int main(void) { return answer(41) == 41 + OFFSET ? 0 : 1; }\n");
    let options = crust::native::Options {
        debug: true,
        include_dirs: vec![relative.join("include")],
        link_inputs: vec![relative.join("target/crust/shared/shared.lib")],
        runtime_libraries: vec![relative.join("target/crust/shared/shared.dll")],
        ..Default::default()
    };
    let output = relative.join("relative.exe");
    crust::native::compile_file_with_options(&relative.join("relative.c"), &output, &options)
        .unwrap();
    success(&Command::new(output).output().unwrap());
}

#[test]
fn renamed_c_functions_can_use_crust_keywords_and_builtin_names() {
    let fixture = Fixture::new();
    fixture.manifest("[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nsources=['native.c']\n");
    fixture.write(
        "native.c",
        r#"
        int destroy(int value) { return value + 1; }
        int move(int value) { return value + 2; }
        int print(int value) { return value + 3; }
        typedef int (*Callback)(int);
        Callback native_callback(void) { return destroy; }
    "#,
    );
    fixture.write(
        "main.cr",
        r#"
        extern int native_destroy(int value) = "destroy";
        extern int native_move(int value) = "move";
        extern int native_print(int value) = "print";
        typedef int (*Callback)(int);
        extern Callback get_callback() = "native_callback";
        int main() {
            unsafe {
                print(native_destroy(41)); print(native_move(40)); print(native_print(39));
                Callback callback = native_destroy; print(callback(41)); print(get_callback()(41));
            }
            return 0;
        }
    "#,
    );
    let output = fixture.invoke(&["run"]);
    success(&output);
    assert_eq!(text(&output), "42\n42\n42\n42\n42\n");
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

#[test]
fn local_path_dependencies_deduplicate_diamonds_and_keep_builds_in_the_consumer() {
    let fixture = Fixture::new();
    fixture.manifest("[dependencies]\nleft={path='left'}\nright={path='right'}\n[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nlink-targets=['left:api', 'right:api']\n");
    fixture.write("main.cr", "import \"common/helpers.cr\"; extern int left_value(); extern int right_value(); int main() { unsafe { print(twice<int>(left_value() + right_value())); } return 0; }");
    fixture.write(
        "common/helpers.cr",
        "T twice<T>(T value) { return value + value; }",
    );
    fixture.write("common/value.h", "#define VALUE 10\n");
    fixture.write(
        "common/value.c",
        "#include \"value.h\"\nint common_value(void) { return VALUE; }\n",
    );
    fixture.write("common/api.cr", "extern int common_value();");
    fixture.write("common/crust.toml", "[project]\nname='common'\n[[targets]]\nname='api'\nkind='static-library'\nentry='api.cr'\n[targets.native]\nsources=['value.c']\n");
    for (side, offset) in [("left", 1), ("right", 2)] {
        fixture.write(&format!("{side}/crust.toml"), &format!("[project]\nname='{side}'\n[dependencies]\ncommon={{path='../common'}}\n[[targets]]\nname='api'\nkind='static-library'\nentry='api.cr'\n[targets.native]\nlink-targets=['common:api']\n"));
        fixture.write(&format!("{side}/api.cr"), &format!("extern int common_value(); extern int {side}_value() {{ unsafe {{ return common_value() + {offset}; }} }}"));
    }
    let project = crust::project::Project::load(&fixture.directory).unwrap();
    assert_eq!(project.manifests.len(), 4);
    assert_eq!(project.targets.len(), 4);
    assert_eq!(project.select(None, true).unwrap()[0].name, "app");
    assert_eq!(project.metadata().unwrap().targets.len(), 1);
    let left_common = project.select(Some("left:common:api"), true).unwrap()[0];
    let right_common = project.select(Some("right:common:api"), true).unwrap()[0];
    assert!(std::ptr::eq(left_common, right_common));
    let native = project.resolved_native(&project.targets[0]).unwrap();
    assert_eq!(native.link_inputs.len(), 3);
    assert_eq!(
        native.link_inputs.last().unwrap(),
        &left_common
            .kind
            .link_path(&project.output_base(left_common))
            .unwrap()
    );
    success(&fixture.invoke(&["run"]));
    let output = Command::new(executable(&fixture)).output().unwrap();
    success(&output);
    assert_eq!(text(&output), "46\n");
    fixture.write("common/value.h", "#define VALUE 20\n");
    success(&fixture.invoke(&["run"]));
    assert_eq!(
        text(&Command::new(executable(&fixture)).output().unwrap()),
        "86\n"
    );
    fixture.write(
        "common/value.c",
        "#include \"value.h\"\nint common_value(void) { return VALUE + 1; }\n",
    );
    success(&fixture.invoke(&["run"]));
    assert_eq!(
        text(&Command::new(executable(&fixture)).output().unwrap()),
        "90\n"
    );
    for dependency in ["left", "right", "common"] {
        assert!(!fixture.directory.join(dependency).join("target").exists());
    }
    let output = fixture.invoke(&["metadata", "--target", "right:common:api"]);
    success(&output);
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(metadata["targets"].as_array().unwrap().len(), 1);
    assert_eq!(metadata["targets"][0]["name"], left_common.name);
}

#[test]
fn local_path_dependencies_report_configuration_and_graph_errors() {
    let fixture = Fixture::new();
    fixture.write("main.cr", "int main() { return 0; }");
    let app = "[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n";
    for (dependencies, expected) in [
        (
            "[dependencies]\nsupport={path='missing'}\n",
            "dependency 'support'",
        ),
        ("[dependencies]\nsupport={url='remote'}\n", "unknown field"),
        (
            "[dependencies]\n'bad:name'={path='.'}\n",
            "dependency names",
        ),
        (
            "[dependencies]\nSUPPORT={path='support'}\nsupport={path='support'}\n",
            "dependency names",
        ),
    ] {
        fixture.write("support/api.cr", "extern int answer();");
        fixture.write("support/crust.toml", "[project]\nname='support'\n[[targets]]\nname='api'\nkind='static-library'\nentry='api.cr'\n");
        fixture.manifest(&format!("{dependencies}{app}"));
        let error = crust::project::Project::load(&fixture.directory).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
    for link in ["support:missing", "unknown:api", "support:api"] {
        fixture.manifest(&format!("[dependencies]\nsupport={{path='support'}}\n{app}[targets.native]\nlink-targets=['{link}']\n"));
        fixture.write("support/api.cr", "int main() { return 0; }");
        fixture.write("support/crust.toml", "[project]\nname='support'\n[[targets]]\nname='api'\nkind='executable'\nentry='api.cr'\n");
        let error = crust::project::Project::load(&fixture.directory).unwrap_err();
        assert!(
            error.contains(if link == "support:api" {
                "must be a static-library or shared-library"
            } else {
                "unknown target"
            }),
            "{error}"
        );
    }
    fixture.manifest(&format!(
        "[dependencies]\nsupport={{path='support'}}\n{app}"
    ));
    fixture.write("support/crust.toml", "[project]\nname='support'\n[dependencies]\nparent={path='..'}\n[[targets]]\nname='api'\nkind='executable'\nentry='api.cr'\n");
    let error = crust::project::Project::load(&fixture.directory).unwrap_err();
    assert!(error.contains("project dependency cycle"), "{error}");
}

#[test]
fn local_shared_dependencies_supply_runtime_files() {
    let fixture = Fixture::new();
    fixture.manifest("[dependencies]\nsupport={path='support'}\n[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n[targets.native]\nlink-targets=['support:api']\n");
    fixture.write(
        "main.cr",
        "extern int answer(); int main() { unsafe { print(answer()); } return 0; }",
    );
    fixture.write("support/api.cr", "extern int answer() { return 42; }");
    fixture.write("support/crust.toml", "[project]\nname='support'\n[[targets]]\nname='api'\nkind='shared-library'\nentry='api.cr'\n");
    let output = fixture.invoke(&["run"]);
    success(&output);
    assert_eq!(text(&output), "42\n");
    let project = crust::project::Project::load(&fixture.directory).unwrap();
    let metadata = project.metadata().unwrap();
    assert_eq!(metadata.targets[0].runtime_libraries.len(), 1);
    assert!(
        metadata.targets[0].runtime_libraries[0]
            .starts_with(project.directory.join("target/crust/dependencies"))
    );
    assert!(!fixture.directory.join("support/target").exists());
}
