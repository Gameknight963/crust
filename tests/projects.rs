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
