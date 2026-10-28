mod common;
use common::compiler as crust;

use std::path::Path;

fn sources(body: &str) -> crust::source::SourceSet {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let entry = root.join("target/library-example.cr");
    let text = format!(
        "import \"../lib/box.cr\"; import \"../lib/buffer.cr\"; import \"../lib/vector.cr\"; import \"../lib/result.cr\"; {body}"
    );
    return crust::source::SourceSet::load_with(&entry, |path| {
        if path.ends_with("library-example.cr") {
            return Ok(text.clone());
        }
        return std::fs::read_to_string(path);
    })
    .unwrap();
}

#[test]
fn fallible_buffers_distinguish_empty_allocation_failure_and_capacity_overflow() {
    let source = sources(
        r#"
        extern void crust_test_fail_allocations_after(size_t successes);
        int main() {
            unsafe { crust_test_fail_allocations_after(0); }
            Result<Buffer<int>, AllocationFailure<int>> empty = Buffer<int>::try_create(0, 7);
            print(empty.is_ok());
            Result<Buffer<int>, AllocationFailure<int>> failed = Buffer<int>::try_create(2, 42);
            AllocationFailure<int> failure = move(failed).take_error();
            print((int)failure.reason()); print(move(failure).take_value());
            Result<Buffer<int>, AllocationFailure<int>> large = Buffer<int>::try_create(~(size_t)0, 9);
            AllocationFailure<int> overflow = move(large).take_error();
            print((int)overflow.reason()); print(move(overflow).take_value());
            unsafe { crust_test_fail_allocations_after(~(size_t)0); }
            Result<Buffer<int>, AllocationFailure<int>> success = Buffer<int>::try_create(2, 3);
            Buffer<int> values = move(success).take_value();
            print(values.count()); print(values.at(1));
            return 0;
        }
    "#,
    );
    let output = common::tracked_generated_c(&source.compile().unwrap(), true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "1\n0\n42\n1\n9\n2\n3\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace('\r', ""),
        "drop:bytes:8\n"
    );
}

#[test]
fn failed_box_allocation_returns_the_owner_for_retry() {
    let source = sources(
        r#"
        extern void crust_test_fail_allocations_after(size_t successes);
        int main() {
            Box<int> original = Box<int>::create(42);
            unsafe { crust_test_fail_allocations_after(0); }
            Result<Box<Box<int>>, AllocationFailure<Box<int>>> first =
                Box<Box<int>>::try_create(move(original));
            print(first.is_ok());
            AllocationFailure<Box<int>> failure = move(first).take_error();
            print((int)failure.reason());
            Box<int> recovered = move(failure).take_value();
            print(recovered.get());
            unsafe { crust_test_fail_allocations_after(~(size_t)0); }
            Result<Box<Box<int>>, AllocationFailure<Box<int>>> second =
                Box<Box<int>>::try_create(move(recovered));
            print(second.is_ok());
            Box<Box<int>> value = move(second).take_value();
            print(value.get().get());
            return 0;
        }
    "#,
    );
    let output = common::tracked_generated_c(&source.compile().unwrap(), true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "0\n0\n42\n1\n42\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr)
            .matches("drop:bytes:")
            .count(),
        2
    );
}

#[test]
fn failed_vector_growth_preserves_elements_and_returns_rejected_owners() {
    let source = sources(
        r#"
        extern void crust_test_fail_allocations_after(size_t successes);
        int main() {
            Vector<Box<int>> values = Vector<Box<int>>::create();
            values.reserve(2);
            values.push(Box<int>::create(10)); values.push(Box<int>::create(20));
            Box<int> original = Box<int>::create(15);
            unsafe { crust_test_fail_allocations_after(0); }
            Result<Unit, AllocationFailure<Box<int>>> inserted = values.try_insert(1, move(original));
            print(inserted.is_ok()); print(values.count());
            print(values.at(0).get()); print(values.at(1).get());
            AllocationFailure<Box<int>> failure = move(inserted).take_error();
            Box<int> recovered = move(failure).take_value();
            Result<Unit, AllocError> reserved = values.try_reserve(4);
            print((int)reserved.error_value()); print(values.count());
            Result<Unit, AllocError> overflow = values.try_reserve(~(size_t)0);
            print((int)overflow.error_value());
            unsafe { crust_test_fail_allocations_after(~(size_t)0); }
            Result<Unit, AllocationFailure<Box<int>>> retried = values.try_push(move(recovered));
            print(retried.is_ok()); print(values.count()); print(values.at(2).get());
            return 0;
        }
    "#,
    );
    let output = common::tracked_generated_c(&source.compile().unwrap(), true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "0\n2\n10\n20\n0\n2\n1\n1\n3\n15\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr)
            .matches("drop:bytes:")
            .count(),
        5
    );
}

#[test]
fn dropping_an_allocation_error_destroys_the_rejected_owner_once() {
    let source = sources(
        r#"
        extern void crust_test_fail_allocations_after(size_t successes);
        int main() {
            Vector<Box<int>> values = Vector<Box<int>>::create();
            Box<int> original = Box<int>::create(42);
            unsafe { crust_test_fail_allocations_after(0); }
            Result<Unit, AllocationFailure<Box<int>>> result = values.try_push(move(original));
            print(result.is_ok());
            return 0;
        }
    "#,
    );
    let output = common::tracked_generated_c(&source.compile().unwrap(), true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "0\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace('\r', ""),
        "drop:bytes:4\n"
    );
}

#[test]
fn shared_library_owns_nested_values_and_releases_each_allocation() {
    let source = sources(
        "int main() { Vector<Box<int>> values = Vector<Box<int>>::create(); values.push(Box<int>::create(42)); print(values.at(0).get()); Box<int> last = values.pop(); print(move(last).take()); return 0; }",
    );
    let output = common::tracked_generated_c(&source.compile().unwrap(), true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n42\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace('\r', ""),
        format!(
            "drop:bytes:4\ndrop:bytes:{}\n",
            std::mem::size_of::<usize>() * 2
        )
    );
}

#[test]
fn shared_library_borrows_prevent_owner_mutation_and_moves() {
    for statement in ["values.clear();", "Vector<Box<int>> other = move(values);"] {
        let body = format!(
            "int main() {{ Vector<Box<int>> values = Vector<Box<int>>::create(); values.push(Box<int>::create(42)); const Box<int>& view = values.at(0); {statement} print(view.get()); return 0; }}"
        );
        assert!(sources(&body).check().is_err(), "accepted {statement}");
    }
}

#[test]
fn container_slices_keep_their_storage_alive_and_allow_checked_mutation() {
    let source = sources(
        "int sum(const int[]& values) { int total = 0; for (size_t i = 0; i < values.length; i++) { total += values[i]; } return total; } int main() { Vector<int> values = Vector<int>::create(); print(values.view().length); values.push(10); values.push(20); { int[]& view = values.view_mut(); view[0] = 40; print(sum(view)); } print(sum(values.view())); return 0; }",
    );
    let output = common::tracked_generated_c(&source.compile().unwrap(), true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "0\n60\n60\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace('\r', ""),
        "drop:bytes:8\n"
    );
    for statement in [
        "values.push(3);",
        "values.clear();",
        "Vector<int> other = move(values);",
    ] {
        let body = format!(
            "int main() {{ Vector<int> values = Vector<int>::create(); values.push(42); const int[]& view = values.view(); {statement} print(view[0]); return 0; }}"
        );
        assert!(sources(&body).check().is_err(), "accepted {statement}");
    }
}

#[test]
fn replacing_an_owner_through_a_slice_destroys_it_once() {
    let source = sources(
        "int main() { Vector<Box<int>> values = Vector<Box<int>>::create(); values.push(Box<int>::create(10)); { Box<int>[]& view = values.view_mut(); view[0] = Box<int>::create(20); print(view[0].get()); } print(values.at(0).get()); return 0; }",
    );
    let output = common::tracked_generated_c(&source.compile().unwrap(), true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "20\n20\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace('\r', ""),
        format!(
            "drop:bytes:4\ndrop:bytes:4\ndrop:bytes:{}\n",
            std::mem::size_of::<usize>() * 2
        )
    );
}

#[test]
fn results_borrow_and_extract_owning_payloads() {
    let source = sources(
        "int main() { Result<Box<int>, int> value = Result<Box<int>, int>::success(Box<int>::create(42)); print(value.is_ok()); print(value.value().get()); Box<int> payload = move(value).take_value(); print(payload.get()); Result<Box<int>, int> failure = Result<Box<int>, int>::failure(7); print(failure.is_ok()); print(move(failure).take_error()); return 0; }",
    );
    let output = common::tracked_generated_c(&source.compile().unwrap(), true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "1\n42\n42\n0\n7\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace('\r', ""),
        "drop:bytes:4\n"
    );
}
