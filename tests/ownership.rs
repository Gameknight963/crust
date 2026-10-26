use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_CASE: AtomicUsize = AtomicUsize::new(0);

#[test]
fn rejects_copying_and_using_moved_owners_across_control_flow() {
    let prelude = "int take(BoxInt owner) { return *owner; } ";
    let cases = [
        ("BoxInt a = box_int(1); BoxInt b = a;", "cannot be copied"),
        ("BoxInt a = box_int(1); take(a);", "cannot be copied"),
        (
            "BoxInt a = box_int(1); take(move(a)); print(*a);",
            "has been moved",
        ),
        (
            "BoxInt a = box_int(1); *a = take(move(a));",
            "has been moved",
        ),
        (
            "BoxInt a = box_int(1); take(move(a)); take(move(a));",
            "has been moved",
        ),
        (
            "BoxInt a = box_int(1); if (1) { take(move(a)); } print(*a);",
            "may have been moved",
        ),
        (
            "BoxInt a = box_int(1); 0 && take(move(a)); print(*a);",
            "may have been moved",
        ),
        (
            "BoxInt a = box_int(1); while (1) { take(move(a)); }",
            "restore moved outer owners",
        ),
        (
            "BoxInt a = box_int(1); while (take(move(a))) {}",
            "loop condition cannot consume",
        ),
        ("int a = 1; move(a);", "requires a BoxInt"),
        ("move(box_int(1));", "requires the name"),
        ("BoxInt a = 1;", "found int"),
        ("int a = box_int(1);", "found BoxInt"),
        ("int a = 1; print(*a);", "dereference"),
        ("print(*box_int(1));", "dereference"),
    ];
    for (body, message) in cases {
        let source = format!("{prelude} int main(void) {{ {body} return 0; }}");
        let error = crust::compile(&source).unwrap_err();
        assert!(error.message.contains(message), "{source}: {error}");
    }
}

#[test]
fn accepts_returning_branches_reinitialization_and_restored_loop_owners() {
    crust::check(
        r#"
        int take(BoxInt owner) { return *owner; }
        int main(void) {
            BoxInt a = box_int(1);
            if (0) { take(move(a)); return 0; }
            print(*a);
            if (1) { take(move(a)); }
            a = box_int(2);
            int n = 3;
            while (n > 0) {
                take(move(a));
                a = box_int(n);
                n = n - 1;
            }
            print(*a);
            take(move(a));
            (a) = box_int(3);
            print(*a);
            return 0;
        }
    "#,
    )
    .unwrap();
}

// Instrument the generated C's allocator, rather than inferring cleanup from
// text matching. Real compiled executions verify frees and their scope order.
fn tracked_run(source: &str) -> Output {
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
        static size_t live_allocations = 0;
        static void *tracked_malloc(size_t bytes) {
            void *value = malloc(bytes);
            if (value != NULL) { live_allocations += 1; }
            return value;
        }
        static void tracked_free(void *value) {
            if (value != NULL) {
                fprintf(stderr, "drop:%" PRId32 "\n", *(int32_t *)value);
                live_allocations -= 1;
            }
            free(value);
        }
        #define malloc tracked_malloc
        #define free tracked_free
        #define main crust_test_main
    "#;
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

#[test]
fn frees_owners_on_scope_exit_replacement_discard_and_early_return_in_reverse_order() {
    let output = tracked_run(
        r#"
        BoxInt pass(BoxInt owner) {
            BoxInt local = box_int(20);
            return move(owner);
        }
        int main(void) {
            BoxInt first = box_int(10);
            { BoxInt nested = box_int(30); }
            BoxInt second = pass(move(first));
            second = box_int(40);
            box_int(50);
            BoxInt last = box_int(60);
            print(*second);
            return 0;
        }
    "#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "40\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        "drop:30\ndrop:20\ndrop:10\ndrop:50\ndrop:60\ndrop:40\n"
    );
}

#[test]
fn frees_resources_each_iteration_and_only_on_paths_that_own_them() {
    let output = tracked_run(
        r#"
        int take(BoxInt owner) { return *owner; }
        int main(void) {
            BoxInt owner = box_int(0);
            int n = 1;
            while (n < 10) {
                take(move(owner));
                owner = box_int(n);
                n = n + 1;
            }
            if (n == 10) { take(move(owner)); }
            owner = box_int(42);
            owner = move(owner);
            print(*owner);
            return 0;
        }
    "#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n"
    );
    let drops = String::from_utf8_lossy(&output.stderr);
    assert_eq!(drops.lines().count(), 11);
}

#[test]
fn replacing_through_an_exclusive_borrow_drops_the_old_owner_and_preserves_the_new_one() {
    let output = tracked_run(
        r#"
        void replace(BoxInt^ owner) { *owner = box_int(2); }
        int read(const BoxInt^ owner) { return **owner; }
        int main(void) {
            BoxInt owner = box_int(1);
            replace(^owner);
            print(read(^const owner));
            return 0;
        }
    "#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "2\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        "drop:1\ndrop:2\n"
    );
}
