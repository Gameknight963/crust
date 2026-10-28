pub(super) fn native_source() -> String {
    let native = include_str!("../../lib/native/file.c").replace("#include \"file.h\"", "");
    return format!(
        "{TRACKING}\n{}\n{native}\n{UNDEFINE}",
        include_str!("../../lib/native/file.h")
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
    static FILE* register_file(FILE* handle) {
        if (!registered_check) { atexit(files_closed_at_exit); registered_check = 1; }
        if (handle == NULL) { return NULL; }
        for (int i = 0; i < 64; ++i) {
            if (records[i].handle == NULL) {
                records[i].handle = handle; records[i].direction = 0; records[i].error = 0;
                live_files += 1; return handle;
            }
        }
        abort();
    }
    static FILE* tracked_tmpfile(void) {
        if (fail_next(0)) { return NULL; }
        return register_file(tmpfile());
    }
    static FILE* tracked_fopen(const char* path, const char* mode) {
        if (fail_next(0)) { return NULL; }
        return register_file(fopen(path, mode));
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
    #define fopen tracked_fopen
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
    #undef fopen
    #undef fclose
    #undef fgetc
    #undef fputc
    #undef fflush
    #undef fseek
    #undef ferror
    #undef clearerr
"#;
