use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_CASE: AtomicUsize = AtomicUsize::new(0);

// Instrument the generated C's allocator, rather than inferring cleanup from
// text matching. Real compiled executions verify frees and their scope order.
#[allow(dead_code)]
pub fn tracked_run(source: &str) -> Output {
    return run_with_tracking(source, false);
}

// Each integration test compiles this module independently.
#[allow(dead_code)]
pub fn tracked_run_sizes(source: &str) -> Output {
    return run_with_tracking(source, true);
}

fn run_with_tracking(source: &str, sizes: bool) -> Output {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("ownership-tests");
    fs::create_dir_all(&directory).unwrap();
    let id = NEXT_CASE.fetch_add(1, Ordering::Relaxed);
    let path = directory.join(format!("case_{}_{}.c", std::process::id(), id));
    let executable = path.with_extension(std::env::consts::EXE_EXTENSION);
    let generated = crust::compile(source).unwrap();
    let prefix = r#"
        #include <stdlib.h>
        #include <stdio.h>
        #include <stdint.h>
        #include <inttypes.h>
        #include <string.h>
        struct tracked_allocation { void *pointer; size_t size; struct tracked_allocation *next; };
        static struct tracked_allocation *allocations = NULL;
        static size_t live_allocations = 0;
        static void *tracked_malloc(size_t bytes) {
            void *value = malloc(bytes);
            if (value != NULL) {
                struct tracked_allocation *record = malloc(sizeof(*record));
                if (record == NULL) { free(value); return NULL; }
                record->pointer = value; record->size = bytes; record->next = allocations;
                allocations = record;
                live_allocations += 1;
            }
            return value;
        }
        static void tracked_free(void *value) {
            if (value != NULL) {
                struct tracked_allocation **link = &allocations;
                while (*link != NULL && (*link)->pointer != value) { link = &(*link)->next; }
                if (*link == NULL) { fprintf(stderr, "invalid free\n"); exit(98); }
                struct tracked_allocation *record = *link;
                *link = record->next;
                if (CRUST_TEST_LOG_SIZES || record->size < sizeof(int32_t)) {
                    fprintf(stderr, "drop:bytes:%zu\n", record->size);
                } else {
                    int32_t contents;
                    memcpy(&contents, value, sizeof(contents));
                    fprintf(stderr, "drop:%" PRId32 "\n", contents);
                }
                free(record);
                live_allocations -= 1;
            }
            free(value);
        }
        #define malloc tracked_malloc
        #define free tracked_free
        #define main crust_test_main
    "#
    .replace("CRUST_TEST_LOG_SIZES", if sizes { "1" } else { "0" });
    let suffix = r#"
        #undef main
        #undef malloc
        #undef free
        int main(void) {
            int result = crust_test_main();
            if (live_allocations != 0) {
                fprintf(stderr, "leaked %zu allocation(s)\n", live_allocations);
                return 99;
            }
            return result;
        }
    "#;
    fs::write(&path, format!("{prefix}\n{generated}\n{suffix}")).unwrap();
    crust::native::compile_file(&path, &executable).unwrap();
    return Command::new(executable).output().unwrap();
}
