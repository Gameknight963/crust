#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn fixture() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/dll-import-tests")
        .join(std::process::id().to_string())
        .join(NEXT.fetch_add(1, Ordering::Relaxed).to_string());
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("native.c"),
        r#"
        __declspec(dllexport) int native_count = 40;
        __declspec(dllexport) const int native_limit = 100;
        __declspec(dllexport) int native_bump(void) { return ++native_count; }
        static int twice(int n) { return n * 2; }
        __declspec(dllexport) int (*native_callback)(int) = twice;
    "#,
    )
    .unwrap();
    success(
        &Command::new("clang")
            .args(["-std=c11", "-O2", "-c"])
            .arg(root.join("native.c"))
            .arg("-o")
            .arg(root.join("native.obj"))
            .output()
            .unwrap(),
    );
    success(
        &Command::new("lld-link")
            .args(["/dll", "/noentry"])
            .arg(format!("/out:{}", root.join("native.dll").display()))
            .arg(format!("/implib:{}", root.join("native.lib").display()))
            .arg(root.join("native.obj"))
            .output()
            .unwrap(),
    );
    return root;
}

fn execute(root: &Path, source: &str) -> Output {
    let generated = crust::compile(source).unwrap();
    let path = root.join("main.c");
    let executable = root.join("main.exe");
    std::fs::write(&path, generated).unwrap();
    crust::native::compile_file_with_options(
        &path,
        &executable,
        &crust::native::Options {
            link_inputs: vec![root.join("native.lib")],
            ..Default::default()
        },
    )
    .unwrap();
    return Command::new(executable).output().unwrap();
}

#[test]
fn imported_dll_data_functions_callbacks_and_compatible_aliases_execute() {
    let root = fixture();
    let output = execute(
        &root,
        r#"
        extern __declspec(dllimport) int native_count;
        extern int alias = "native_count";
        __declspec(dllimport) extern const int native_limit;
        extern __declspec(dllimport) int native_bump();
        typedef int (*Callback)(int);
        typedef int (*Bump)();
        extern __declspec(dllimport) Callback native_callback;
        int main() {
            unsafe {
                native_count = 40;
                print(native_bump());
                int* pointer = &alias; *pointer += 1;
                print(native_count); print(native_limit); print(native_callback(21));
                Bump bump = native_bump; print(bump());
            }
            return 0;
        }
    "#,
    );
    success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "41\n42\n100\n42\n43\n"
    );
}

#[test]
fn header_generated_dll_imports_preserve_data_storage_and_const_permissions() {
    let root = fixture();
    let header = root.join("native.h");
    std::fs::write(
        &header,
        r#"
        typedef int (*Callback)(int);
        __declspec(dllimport) extern int native_count;
        __declspec(dllimport) extern const int native_limit;
        __declspec(dllimport) extern Callback native_callback;
        __declspec(dllimport) int native_bump(void);
    "#,
    )
    .unwrap();
    let bindings = crust::bindings::generate(
        &header,
        &crust::bindings::Options {
            functions: vec!["native_bump".to_owned()],
            variables: ["native_count", "native_limit", "native_callback"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(bindings.matches("__declspec(dllimport)").count(), 4);
    let source = format!(
        "{bindings} int main() {{ unsafe {{ print(native_bump()); print(native_limit); print(native_callback(21)); }} return 0; }}"
    );
    let output = execute(&root, &source);
    success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "41\n100\n42\n"
    );
    let invalid = format!("{bindings} int main() {{ unsafe {{ native_limit = 2; }} return 0; }}");
    assert!(crust::check(&invalid).is_err());
}

#[test]
fn dllimport_requires_extern_and_supported_attribute_syntax() {
    for declaration in [
        "__declspec(dllimport) int value;",
        "extern __declspec(dllexport) int value;",
        "extern __declspec(dllimport int value;",
    ] {
        let source = format!("{declaration} int main() {{ return 0; }}");
        assert!(crust::check(&source).is_err(), "{source}");
    }
    let root = fixture();
    let header = root.join("exported.h");
    std::fs::write(&header, "__declspec(dllexport) extern int value;").unwrap();
    let error = crust::bindings::generate(
        &header,
        &crust::bindings::Options {
            variables: vec!["value".to_owned()],
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error.contains("consumer configuration"));
}
