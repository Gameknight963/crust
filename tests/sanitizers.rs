mod common;

fn enabled(name: &str) -> bool {
    return std::env::var("CRUST_TEST_SANITIZERS")
        .is_ok_and(|value| value.split(',').any(|item| item == name));
}

#[test]
fn address_sanitizer_detects_invalid_raw_storage_access() {
    if !enabled("address") {
        return;
    }
    let output = common::tracked_run(
        "extern void* malloc(size_t bytes); extern void free(void* p); int main() { unsafe { int* pointer=(int*)malloc(sizeof(int)); pointer[0]=1; print(pointer[1]); free(pointer); } return 0; }",
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("AddressSanitizer"));
}

#[test]
fn undefined_behavior_sanitizer_detects_overflow_in_native_code() {
    if !enabled("undefined") {
        return;
    }
    let mut generated = crust::compile("extern int native_overflow(); int main() { unsafe { print(native_overflow()); } return 0; }").unwrap();
    generated.push_str(
        "int native_overflow(void) { volatile int32_t value=INT32_MAX; return value+1; }",
    );
    let output = common::tracked_generated_c(&generated, false);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("signed integer overflow"));
}
