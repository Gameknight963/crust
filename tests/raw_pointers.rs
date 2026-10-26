mod common;

fn run(source: &str, stdout: &str, drops: &str) {
    let output = common::tracked_run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        stdout
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        drops
    );
}

#[test]
fn c_allocation_can_be_wrapped_in_a_safe_move_only_owner() {
    run(include_str!("../examples/c_owner.cr"), "42\n", "drop:42\n");
}

#[test]
fn raw_pointers_support_nested_indirection_and_safe_borrow_bindings() {
    run(
        r#"
        typedef int* Pointer;
        int main() {
            int value = 1;
            unsafe {
                Pointer p = &value;
                int** pp = &p;
                **pp = 2;
                {
                    Pointer^ slot = ^p;
                    **slot += 1;
                }
                const int* view = p;
                print(*view); print(*p);
                if (p != view || view != p) { return 1; }
                const void* opaque = view;
                if (p != opaque || opaque != p) { return 2; }
                uintptr_t bits = (uintptr_t)p;
                int* restored = (int*)bits;
                print(restored == p);
                int* empty = 0;
                print(!empty); print(p && !empty); print(0 == empty);
            }
            print(value);
            return 0;
        }
    "#,
        "3\n3\n1\n1\n1\n1\n3\n",
        "",
    );
}

#[test]
fn calls_c_runtime_functions_with_native_char_pointers_and_unnamed_parameters() {
    run(
        r#"
        extern int puts(const char*);
        extern size_t strlen(const char*);
        int main() {
            char text[6] = "Crust";
            unsafe {
                puts(&text[0]);
                print(strlen(&text[0]));
            }
            return 0;
        }
    "#,
        "Crust\n5\n",
        "",
    );
}

#[test]
fn unsafe_blocks_scope_owners_and_preserve_cleanup_on_return_and_loop_control() {
    run(
        r#"
        int get() {
            unsafe {
                BoxInt owner = box_int(7);
                int* pointer = &*owner;
                return *pointer;
            }
        }
        int main() {
            for (int i = 0; i < 2; i += 1) {
                unsafe {
                    BoxInt owner = box_int(i);
                    if (i == 0) { continue; }
                    break;
                }
            }
            print(get());
            return 0;
        }
    "#,
        "7\n",
        "drop:0\ndrop:1\ndrop:7\n",
    );
}

#[test]
fn raw_pointer_operations_and_c_calls_require_explicit_unsafe_contexts() {
    for source in [
        "int main() { int n = 1; int* p = &n; return 0; }",
        "int read(int* p) { return *p; } int main() { return 0; }",
        "int main() { int* p = 0; *p = 1; return 0; }",
        "int main() { int* p = 0; int^ r = ^*p; return 0; }",
        "int main() { int* p = (int*)0; return 0; }",
        "int main() { int* p = 0; uintptr_t bits = (uintptr_t)p; return 0; }",
        "extern void abort(); int main() { abort(); return 0; }",
    ] {
        let error = crust::check(source).expect_err(source);
        assert!(error.message.contains("unsafe block"), "{source}: {error}");
    }
}

#[test]
fn pointer_type_checks_reject_invalid_casts_mutability_and_c_signatures() {
    for source in [
        "int main() { unsafe { int* p = (int*)box_int(1); } return 0; }",
        "int main() { unsafe { int* p = (int*)42; } return 0; }",
        "int main() { unsafe { int* p = 0; int value = (int)p; } return 0; }",
        "int main() { unsafe { void* p = 0; print(*p); } return 0; }",
        "int main() { unsafe { const int* p = 0; *p = 1; } return 0; }",
        "int main() { unsafe { const int* p = 0; int* q = p; } return 0; }",
        "int main() { unsafe { const int* p = 0; void* q = p; } return 0; }",
        "extern BoxInt make(); int main() { return 0; }",
        "extern void take(int^); int main() { return 0; }",
        "extern void take(int values[2]); int main() { return 0; }",
        "extern int main();",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}

#[test]
fn unevaluated_size_queries_allow_raw_pointer_types_without_accessing_memory() {
    run(
        r#"
        extern int never_called(int*);
        int main() {
            int* pointer = 0;
            print(sizeof(*pointer));
            print(sizeof(never_called(pointer)));
            print(sizeof(int*));
            return 0;
        }
    "#,
        &format!("4\n4\n{}\n", std::mem::size_of::<usize>()),
        "",
    );
}
