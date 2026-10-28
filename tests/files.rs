mod common;

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
    let native = include_str!("../lib/native/file.c").replace("#include \"file.h\"", "");
    let instrumented = format!(
        "{TRACKING}\n{}\n{native}\n{UNDEFINE}\n{generated}",
        include_str!("../lib/native/file.h"),
    );
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

// These hooks run the real C stdio operations and check the update-stream
// sequencing rules. EOF, seek, flush, and close behavior therefore comes from
// the native stream rather than a simulated byte buffer.
const TRACKING: &str = r#"
    #include <stdio.h>
    #include <stdlib.h>
    struct file_record { FILE* handle; int direction; int error; };
    static struct file_record records[64];
    static int live_files = 0;
    static int closed_files = 0;
    static int fail_operation = -1;
    static int registered_check = 0;
    static void files_closed_at_exit(void) {
        if (live_files != 0) { fprintf(stderr, "leaked %d file(s)\n", live_files); abort(); }
    }
    static struct file_record* record_for(FILE* handle) {
        for (int i = 0; i < 64; ++i) {
            if (records[i].handle == handle && handle != NULL) { return &records[i]; }
        }
        fprintf(stderr, "operation on a closed file\n"); abort();
    }
    static int fail_next(int operation) {
        if (fail_operation != operation) { return 0; }
        fail_operation = -1; return 1;
    }
    void test_file_fail_next(int operation) { fail_operation = operation; }
    int test_files_live(void) { return live_files; }
    int test_files_closed(void) { return closed_files; }
    static FILE* tracked_tmpfile(void) {
        if (!registered_check) { atexit(files_closed_at_exit); registered_check = 1; }
        if (fail_next(0)) { return NULL; }
        FILE* handle = tmpfile();
        if (handle == NULL) { fprintf(stderr, "could not create test stream\n"); abort(); }
        for (int i = 0; i < 64; ++i) {
            if (records[i].handle == NULL) {
                records[i].handle = handle; records[i].direction = 0; records[i].error = 0;
                live_files += 1; return handle;
            }
        }
        abort();
    }
    static int tracked_fclose(FILE* handle) {
        struct file_record* record = record_for(handle);
        record->handle = NULL; live_files -= 1; closed_files += 1;
        int result = fclose(handle);
        return fail_next(1) ? EOF : result;
    }
    static int tracked_fgetc(FILE* handle) {
        struct file_record* record = record_for(handle);
        if (record->direction == 2) { fprintf(stderr, "read after unsynchronized write\n"); abort(); }
        record->direction = 1;
        if (fail_next(4)) { record->error = 1; return EOF; }
        return fgetc(handle);
    }
    static int tracked_fputc(int value, FILE* handle) {
        struct file_record* record = record_for(handle);
        if (record->direction == 1) { fprintf(stderr, "write after unsynchronized read\n"); abort(); }
        record->direction = 2;
        if (fail_next(5)) { record->error = 1; return EOF; }
        return fputc(value, handle);
    }
    static int tracked_fflush(FILE* handle) {
        struct file_record* record = record_for(handle);
        if (record->direction == 1) { fprintf(stderr, "fflush after input\n"); abort(); }
        if (fail_next(2)) { return EOF; }
        int result = fflush(handle);
        if (result == 0) { record->direction = 0; }
        return result;
    }
    static int tracked_fseek(FILE* handle, long offset, int origin) {
        struct file_record* record = record_for(handle);
        if (fail_next(3)) { return -1; }
        int result = fseek(handle, offset, origin);
        if (result == 0) { record->direction = 0; }
        return result;
    }
    static int tracked_ferror(FILE* handle) { return record_for(handle)->error || ferror(handle); }
    static void tracked_clearerr(FILE* handle) { record_for(handle)->error = 0; clearerr(handle); }
    #define tmpfile tracked_tmpfile
    #define fclose tracked_fclose
    #define fgetc tracked_fgetc
    #define fputc tracked_fputc
    #define fflush tracked_fflush
    #define fseek tracked_fseek
    #define ferror tracked_ferror
    #define clearerr tracked_clearerr
"#;

const UNDEFINE: &str = r#"
    #undef tmpfile
    #undef fclose
    #undef fgetc
    #undef fputc
    #undef fflush
    #undef fseek
    #undef ferror
    #undef clearerr
"#;
