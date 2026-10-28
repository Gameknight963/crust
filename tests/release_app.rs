mod common;

use std::path::Path;
use std::process::Output;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn input(name: &str, bytes: &[u8]) -> String {
    let relative = format!(
        "target/crustwc-tests/{}_{}_{name}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(&relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
    return relative;
}

fn run(arguments: &[String], budget: Option<usize>) -> Output {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/crustwc/src/main.cr");
    let sources = crust::source::SourceSet::load(&path).unwrap();
    let generated = sources.compile().unwrap();
    let budget = budget.map_or(String::new(), |value| {
        format!("#define CRUST_TEST_ALLOCATION_BUDGET {value}\n")
    });
    let source = format!(
        "{budget}\n{}\n{}\n{generated}",
        common::tracked_file_source(),
        include_str!("../examples/crustwc/native/console.c")
    );
    return common::tracked_generated_c_with_args(&source, true, arguments);
}

fn diagnostics(output: &Output) -> String {
    return String::from_utf8_lossy(&output.stderr)
        .replace('\r', "")
        .lines()
        .filter(|line| !line.starts_with("drop:bytes:"))
        .map(|line| format!("{line}\n"))
        .collect();
}

#[test]
fn project_driver_builds_the_app_and_forwards_literal_option_names_as_paths() {
    let relative = input("directory.marker", b"");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let directory = root.join(&relative).with_extension("directory");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("--utf8"), b"one\n").unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_crust"))
        .args(["run", "--project"])
        .arg(root.join("examples/crustwc"))
        .args(["--", "--", "--utf8"])
        .current_dir(directory)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "1 4 --utf8\n"
    );
}

#[test]
fn release_app_counts_binary_files_and_unicode_scalars_across_multiple_inputs() {
    let binary = input("binary file.bin", b"A\0\xff\nB\r\n");
    let text = input("unicode.txt", "A\u{e9}\u{1f600}\n".as_bytes());
    let output = run(&[binary.clone(), text.clone()], None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        format!("2 7 {binary}\n1 8 {text}\n")
    );
    assert_eq!(diagnostics(&output), "");
    let output = run(&["--utf8".to_owned(), text.clone()], None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        format!("1 8 4 {text}\n")
    );
}

#[test]
fn release_app_continues_after_missing_files_and_invalid_utf8() {
    let invalid = input("invalid.bin", b"\xff");
    let valid = input("valid.txt", b"ok\n");
    let missing = format!("{valid}.missing");
    let output = run(
        &[
            "--utf8".to_owned(),
            missing.clone(),
            invalid.clone(),
            valid.clone(),
        ],
        None,
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        format!("1 3 3 {valid}\n")
    );
    assert_eq!(
        diagnostics(&output),
        format!("{missing}: could not open file\n{invalid}: invalid UTF-8\n")
    );
}

#[test]
fn release_app_reports_syntax_errors_and_handles_help_without_input_files() {
    let output = run(&[], None);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(diagnostics(&output), "no input files\n");
    let output = run(&["--unknown".to_owned()], None);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(diagnostics(&output), "--unknown: unknown option\n");
    let output = run(&["--help".to_owned()], None);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("Usage: crustwc"));
    assert_eq!(diagnostics(&output), "");
}

#[test]
fn release_app_handles_allocation_failure_without_leaking_buffers_or_file_handles() {
    let path = input("allocation.txt", b"data\n");
    let output = run(&["--utf8".to_owned(), path.clone()], Some(0));
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(diagnostics(&output), "allocation failed\n");
    for budget in [1, 2] {
        let output = run(&["--utf8".to_owned(), path.clone()], Some(budget));
        assert_eq!(
            output.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(diagnostics(&output), format!("{path}: allocation failed\n"));
    }
}
