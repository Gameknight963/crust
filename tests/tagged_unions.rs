mod common;
use common::compiler as crust;

#[test]
fn tagged_payloads_initialize_switch_and_destroy_only_the_active_owner() {
    let source = r#"
        tagged union Value<T> { T item; int error; };
        int read(const Value<int>& value) {
            switch (value) {
                case item: return value.item;
                case error: return value.error;
            }
        }
        int main() {
            Value<int> a = { .item = 42 };
            Value<int> b = { .error = 7 };
            print(read(&a)); print(read(&b));
            Value<Box<int>> owned = { .item = Box<int>::create(10) };
            switch (owned) {
                case item: print(owned.item.get()); break;
                case error: print(owned.error); break;
            }
            Value<Box<int>> error = { .error = 99 };
            return 0;
        }
    "#;
    let output = common::tracked_run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n7\n10\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace('\r', ""),
        "drop:10\n"
    );
}

#[test]
fn inactive_payload_access_terminates_before_reading_storage() {
    let output = common::tracked_run(
        "tagged union Value { int item; int error; }; int main() { Value v = { .error = 7 }; print(v.item); return 0; }",
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("inactive tagged-union payload"));
}

#[test]
fn extraction_and_switch_cases_reject_invalid_programs() {
    for source in [
        "tagged union V { Box<int> ok; int error; }; int main() { V v = {.ok=Box<int>::create(1)}; Box<int> b = move(v.ok); print(v.error); return 0; }",
        "tagged union V { int ok; int error; }; int main() { V v = {.ok=1,.error=2}; return 0; }",
        "tagged union V { int ok; int error; }; int main() { V v = {.ok=1}; switch(v) { case other: break; } return 0; }",
        "tagged union V { int ok; int error; }; int main() { V v = {.ok=1}; switch(v) { case ok: break; case ok: break; } return 0; }",
        "tagged union V { int ok; int error; }; int main() { V v = {.ok=1}; switch(v) { case ok: continue; } return 0; }",
        "tagged union V { int ok; int error; }; int main() { V v = {.ok=1}; int& a=&v.ok; int& b=&v.error; return 0; }",
    ] {
        assert!(crust::compile(source).is_err(), "accepted {source}");
    }
}

#[test]
fn switch_control_flow_preserves_loop_steps_and_local_cleanup() {
    let source = r#"
        tagged union V { int ok; int error; };
        int main() {
            V v = {.ok=1};
            for (int i = 0; i < 3; i++) {
                Box<int> outer = Box<int>::create(i);
                switch(v) {
                    case ok:
                        Box<int> inner = Box<int>::create(10 + i);
                        if (i == 1) continue;
                        print(i);
                        break;
                    default: return 9;
                }
                print(100);
            }
            return 0;
        }
    "#;
    let output = common::tracked_run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "0\n100\n2\n100\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace('\r', ""),
        "drop:10\ndrop:0\ndrop:11\ndrop:1\ndrop:12\ndrop:2\n"
    );
}

#[test]
fn switch_fallthrough_and_partial_dispatch_preserve_order() {
    let output = common::tracked_run(
        "tagged union V { int ok; int error; }; int main() { V a={.ok=1}; switch(a) { case ok: print(1); case error: print(2); } V b={.error=1}; switch(b) { case ok: print(9); } print(3); return 0; }",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "1\n2\n3\n"
    );
}

#[test]
fn case_labels_and_payload_uses_resolve_to_the_same_field() {
    let source = "tagged union V { int ok; int error; }; int main() { V v={.ok=1}; switch(v) { case ok: print(v.ok); break; default: break; } return 0; }";
    let analysis = crust::analyze(source).unwrap();
    let references = analysis.references_at(0, source.find("ok;").unwrap(), true);
    assert_eq!(references.len(), 4);
}
