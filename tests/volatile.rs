mod common;

fn run(source: &str, native: &str, expected: &str) {
    let generated = format!("{}\n{native}", crust::compile(source).unwrap());
    let output = common::tracked_generated_c(&generated, false);
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
fn volatile_globals_aliases_addresses_and_borrowed_views_keep_their_qualifiers() {
    run(
        r#"
        extern volatile int counter = "native_counter";
        extern int volatile alias = "native_counter";
        extern const volatile int limit = "native_limit";
        extern int* volatile slot = "native_slot";
        extern void native_tick();
        void increment(volatile int^ value) { *value += 1; }
        const volatile int^ relay(const volatile int^ value) { return value; }
        int main() {
            unsafe {
                print(counter);
                counter += 1;
                int volatile* pointer = &counter;
                *pointer += 3;
                increment(^counter);
                { const volatile int^ view = relay(^const alias); print(*view); }
                native_tick();
                print(alias);
                print(limit);
                int* normal = slot;
                *normal = 9;
                slot += 0;
                print(*slot);
            }
            return 0;
        }
    "#,
        "volatile int32_t native_counter = 1; const volatile int32_t native_limit = 10; static int32_t plain = 4; int32_t * volatile native_slot = &plain; void native_tick(void) { native_counter += 1; }",
        "1\n6\n7\n10\n9\n",
    );
}

#[test]
fn volatile_field_array_reborrows_and_copyable_storage_operations_preserve_accesses() {
    run(
        r#"
        struct Pair { int left; int values[2]; };
        typedef int volatile* Register;
        int main() {
            Pair pair = { .left = 1, .values = { 2, 3 } };
            unsafe {
                volatile Pair* raw = &pair;
                volatile Pair^ view = ^*raw;
                view->left += 1;
                view->values[0] += 2;
                { volatile int^ field = ^view->left; *field += 3; }
                print(view->left);
                print(view->values[0]);
            }
            int values[2] = { 1, 2 };
            unsafe {
                Register raw = &values[0];
                init(raw + 1, 20);
                raw[1] += 2;
                print(move(raw[1]));
                destroy(raw + 1);
                const volatile void* erased = raw;
            }
            return 0;
        }
    "#,
        "",
        "5\n4\n22\n",
    );
}

#[test]
fn volatile_qualifiers_preserve_alias_loans_and_prevent_implicit_qualification_loss() {
    for source in [
        "extern volatile int value; int main() { unsafe { int* pointer = &value; } return 0; }",
        "extern const volatile int value; int main() { unsafe { const int* pointer = &value; } return 0; }",
        "extern volatile int value; int main() { unsafe { int^ view = ^value; } return 0; }",
        "extern volatile int value; extern int alias = \"value\"; int main() { return 0; }",
        "extern volatile int value; extern volatile int alias = \"value\"; int main() { unsafe { const volatile int^ view = ^const value; alias += 1; print(*view); } return 0; }",
        "extern const volatile int value; int main() { unsafe { value = 1; } return 0; }",
        "int main() { volatile int value = 1; return 0; }",
        "int main() { unsafe { volatile BoxInt* slot = 0; init(slot, box_int(1)); } return 0; }",
        "int main() { unsafe { volatile BoxInt* slot = 0; destroy(slot); } return 0; }",
        "int main() { unsafe { volatile BoxInt* slot = 0; BoxInt value = move(*slot); } return 0; }",
        "int main() { unsafe { volatile BoxInt* slot = 0; *slot = box_int(1); } return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "{source}");
    }
}
