mod common;

fn named_file(contents: &[u8]) -> (std::path::PathBuf, String) {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let relative = format!(
        "target/file-api-tests/file_{}_{}.bin",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(&relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, contents).unwrap();
    let declaration = format!(
        "char path[{}]={};",
        relative.len() + 1,
        serde_json::to_string(&relative).unwrap()
    );
    return (path, declaration);
}

#[test]
fn named_binary_files_enforce_modes_append_and_synchronize_read_write_access() {
    let (path, declaration) = named_file(b"A\0\xff");
    run(
        &r#"
        int main() {
            PATH_DECLARATION
            Result<File,FileError> opened=File::try_open(&path,FileMode::Read);
            File input=move(opened).take_value();
            print(input.read_byte()); print(input.read_byte()); print(input.read_byte());
            Result<Unit,FileError> denied=input.try_write_byte(1u);
            print((int)denied.error_value()); input.close();
            Result<File,FileError> appended=File::try_open(&path,FileMode::Append);
            File output=move(appended).take_value(); output.rewind();
            output.write_byte(67u); output.close();
            Result<File,FileError> update=File::try_open(&path,FileMode::ReadWrite);
            File both=move(update).take_value();
            print(both.read_byte()); print(both.write_byte(90u));
            return 0;
        }
    "#
        .replace("PATH_DECLARATION", &declaration),
        "65\n0\n255\n3\n65\n1\n",
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"AZ\xffC");
    run(
        &r#"
        int main() {
            PATH_DECLARATION
            Result<File,FileError> opened=File::try_open(&path,FileMode::Write);
            File output=move(opened).take_value();
            Result<FileRead,FileError> denied=output.try_read_byte();
            print((int)denied.error_value()); output.write_byte(42u);
            return 0;
        }
    "#
        .replace("PATH_DECLARATION", &declaration),
        "2\n",
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"*");
}

#[test]
fn file_open_errors_reject_unterminated_paths_and_leave_no_live_handles() {
    let (_path, declaration) = named_file(b"data");
    run(
        &r#"
        extern void test_file_fail_next(int operation);
        extern int test_files_live();
        int main() {
            PATH_DECLARATION
            char missing[1]={'A'};
            Result<File,FileError> invalid=File::try_open(&missing,FileMode::Read);
            print((int)invalid.error_value());
            unsafe { test_file_fail_next(0); }
            Result<File,FileError> failed=File::try_open(&path,FileMode::Read);
            print((int)failed.error_value());
            Result<File,FileError> mode=File::try_open(&path,(FileMode)255);
            print((int)mode.error_value());
            char empty[1]={0};
            Result<File,FileError> absent=File::try_open(&empty,FileMode::Read);
            print((int)absent.error_value());
            unsafe { print(test_files_live()); }
            return 0;
        }
    "#
        .replace("PATH_DECLARATION", &declaration),
        "7\n0\n0\n0\n0\n",
    );
}

#[test]
fn file_results_distinguish_read_and_write_errors_and_rewind_clears_status() {
    run(
        r#"
        extern void test_file_fail_next(int operation);
        int main() {
            Result<File,FileError> opened=File::try_temporary();
            File file=move(opened).take_value();
            file.write_byte(42u); file.rewind();
            unsafe { test_file_fail_next(4); }
            Result<FileRead,FileError> read=file.try_read_byte();
            print((int)read.error_value()); print(file.has_error());
            file.rewind(); print(file.has_error());
            print(file.read_byte());
            unsafe { test_file_fail_next(5); }
            Result<Unit,FileError> write=file.try_write_byte(43u);
            print((int)write.error_value()); print(file.has_error());
            Result<Unit,FileError> rewind=file.try_rewind(); print(rewind.is_ok());
            print(file.has_error()); print(file.read_byte());
            return 0;
        }
    "#,
        "2\n1\n0\n42\n3\n1\n1\n0\n42\n",
    );
}

#[test]
fn result_file_operations_distinguish_eof_and_recover_from_native_failures() {
    run(
        r#"
        extern void test_file_fail_next(int operation);
        extern int test_files_live();
        extern int test_files_closed();
        int main() {
            unsafe { test_file_fail_next(0); }
            Result<File,FileError> missing=File::try_temporary();
            print((int)missing.error_value());
            Result<File,FileError> opened=File::try_temporary();
            File file=move(opened).take_value();
            Result<Unit,FileError> written=file.try_write_byte(65u);
            print(written.is_ok());
            unsafe { test_file_fail_next(2); }
            Result<FileRead,FileError> failed=file.try_read_byte();
            print((int)failed.error_value());
            Result<FileRead,FileError> at_end=file.try_read_byte();
            FileRead end=at_end.value();
            switch (end) { case byte: print(99); break; case end: print(0); break; }
            file.rewind();
            Result<FileRead,FileError> read=file.try_read_byte();
            FileRead value=read.value();
            switch (value) { case byte: print(value.byte); break; case end: print(99); break; }
            unsafe { test_file_fail_next(3); }
            Result<Unit,FileError> seek=file.try_write_byte(66u);
            print((int)seek.error_value());
            Result<Unit,FileError> retry=file.try_write_byte(66u);
            print(retry.is_ok());
            unsafe { test_file_fail_next(1); }
            Result<Unit,FileError> closed=file.try_close();
            print((int)closed.error_value()); print(file.is_open());
            Result<Unit,FileError> again=file.try_close(); print(again.is_ok());
            Result<FileRead,FileError> unavailable=file.try_read_byte();
            print((int)unavailable.error_value());
            unsafe { print(test_files_live()); print(test_files_closed()); }
            return 0;
        }
    "#,
        "0\n1\n4\n0\n65\n5\n1\n6\n0\n1\n1\n0\n1\n",
    );
}

#[test]
fn file_bindings_match_the_native_header() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("lib");
    let options = crust::bindings::Options {
        binding_dir: Some(root.join("native")),
        functions: [
            "crust_file_temporary",
            "crust_file_open",
            "crust_file_close",
            "crust_file_read_byte",
            "crust_file_write_byte",
            "crust_file_flush",
            "crust_file_rewind",
            "crust_file_sync_position",
            "crust_file_eof",
            "crust_file_error",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        ..Default::default()
    };
    let generated = crust::bindings::generate(&root.join("native/file.h"), &options).unwrap();
    assert_eq!(
        generated.replace("\r\n", "\n"),
        include_str!("../lib/native/file_bindings.cr").replace("\r\n", "\n")
    );
    let sources =
        crust::source::SourceSet::load(&root.join("../examples/file_project/src/main.cr")).unwrap();
    sources.check().unwrap();
}

fn run(program: &str, expected: &str) {
    let source = format!(
        "{}\n{}\n{program}",
        include_str!("../lib/native/file_bindings.cr"),
        include_str!("../lib/file.cr")
            .replace("import \"native/file_bindings.cr\";", "")
            .replace("import \"result.cr\";", include_str!("../lib/result.cr")),
    );
    let generated = crust::compile(&source).unwrap();
    let instrumented = format!("{}\n{generated}", common::tracked_file_source());
    let output = common::tracked_generated_c(&instrumented, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        expected
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn file_example_transfers_a_real_binary_stream_to_its_reader() {
    run(
        &include_str!("../examples/file_project/src/main.cr")
            .replace("import \"../../../lib/file.cr\";", ""),
        "67\n114\n255\n-1\n1\n",
    );
}

#[test]
fn file_methods_synchronize_update_streams_and_reset_eof_after_rewinding() {
    run(
        r#"
        int main() {
            File file = File::temporary();
            if (!file.is_open()) { return 1; }
            print(file.write_byte('A')); print(file.write_byte(255u));
            print(file.rewind()); print(file.read_byte());
            print(file.flush());
            print(file.write_byte('Z'));
            print(file.read_byte()); print(file.eof());
            print(file.rewind()); print(file.eof());
            print(file.read_byte()); print(file.read_byte()); print(file.has_error());
            return 0;
        }
        "#,
        "1\n1\n1\n65\n1\n1\n-1\n1\n1\n0\n65\n90\n0\n",
    );
}

#[test]
fn file_moves_replacement_arrays_conditionals_and_early_returns_close_once() {
    run(
        r#"
        extern int test_files_live();
        extern int test_files_closed();
        File create() { File file = File::temporary(); return move(file); }
        void early(bool stop) {
            File file = create();
            if (stop) { return; }
            file.write_byte(42u);
        }
        int main() {
            File first = create();
            unsafe { print(test_files_live()); }
            {
                File second = move(first);
                unsafe { print(test_files_live()); }
                second = create();
                unsafe { print(test_files_live()); print(test_files_closed()); }
            }
            unsafe { print(test_files_live()); print(test_files_closed()); }
            {
                File files[2] = { create(), create() };
                unsafe { print(test_files_live()); }
            }
            unsafe { print(test_files_closed()); }
            {
                File selected = true ? create() : create();
                unsafe { print(test_files_live()); }
            }
            early(true); early(false);
            unsafe { print(test_files_closed()); }
            File explicit = create();
            print(explicit.close()); print(explicit.close()); print(explicit.is_open());
            print(explicit.read_byte()); print(explicit.write_byte(1u));
            print(explicit.flush()); print(explicit.rewind());
            print(explicit.eof()); print(explicit.has_error());
            unsafe { print(test_files_live()); print(test_files_closed()); }
            return 0;
        }
        "#,
        "1\n1\n1\n1\n0\n2\n2\n4\n1\n7\n1\n1\n0\n-1\n0\n0\n0\n0\n0\n0\n8\n",
    );
}

#[test]
fn file_failures_leave_closed_handles_detached_and_allow_synchronization_retries() {
    run(
        r#"
        extern void test_file_fail_next(int operation);
        extern int test_files_live();
        extern int test_files_closed();
        int main() {
            unsafe { test_file_fail_next(0); }
            File failed = File::temporary();
            print(failed.is_open()); print(failed.close());
            unsafe { print(test_files_closed()); }
            File closed = File::temporary();
            unsafe { test_file_fail_next(1); }
            print(closed.close()); print(closed.is_open()); print(closed.close());
            unsafe { print(test_files_live()); print(test_files_closed()); }
            File file = File::temporary();
            if (!file.is_open()) { return 1; }
            print(file.write_byte(1u));
            unsafe { test_file_fail_next(2); }
            print(file.read_byte());
            print(file.rewind()); print(file.read_byte());
            unsafe { test_file_fail_next(3); }
            print(file.write_byte(2u)); print(file.write_byte(2u));
            print(file.rewind()); print(file.read_byte()); print(file.read_byte());
            return 0;
        }
        "#,
        "0\n1\n0\n0\n0\n1\n0\n1\n1\n-1\n1\n1\n0\n1\n1\n1\n2\n",
    );
}
