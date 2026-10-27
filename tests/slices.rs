mod common;

fn run(source: &str, expected: &str) {
    let output = common::tracked_run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        expected
    );
}

#[test]
fn slices_accept_different_array_lengths_and_check_mutable_access() {
    run(
        r#"
        int sum(const int[]& values) {
            int total = 0;
            for (size_t i = 0; i < values.length; i++) { total += values[i]; }
            return total;
        }
        void increment(int[]& values) {
            for (size_t i = 0; i < values.length; i++) { values[i] += 1; }
        }
        int main() {
            int[3] a = { 1, 2, 3 };
            int[2] b = { 10, 20 };
            increment(&a);
            print(sum(&a));
            print(sum(&b));
            int[]& view = &b;
            print(view.length);
            view[0] = 40;
            const int[]& shared = &view;
            print(sum(shared));
            return 0;
        }
    "#,
        "9\n30\n2\n60\n",
    );
}

#[test]
fn slices_preserve_returned_borrows_and_generic_element_types() {
    run(
        r#"
        const T[]& identity<T>(const T[]& values) { return values; }
        int[]& mutable_view(int[]& values) { return values; }
        const int[]& from_array(const int[2]& values) { return values; }
        int main() {
            int[2] values = { 3, 4 };
            mutable_view(&values)[0] = 20;
            print(identity<int>(&values)[0]);
            print(from_array(&values).length);
            char[3] letters = { 'a', 'b', 'c' };
            print(identity<char>(&letters)[2]);
            return 0;
        }
    "#,
        "20\n2\n99\n",
    );
}

#[test]
fn slices_borrow_owning_elements_without_copying_or_dropping_them() {
    let source = r#"
        void show(const BoxInt[]& values) {
            for (size_t i = 0; i < values.length; i++) { print(*values[i]); }
        }
        int main() {
            BoxInt[2] values = { box_int(10), box_int(20) };
            show(&values);
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
        "10\n20\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace('\r', ""),
        "drop:20\ndrop:10\n"
    );
}

#[test]
fn slices_reject_conflicting_access_escaping_storage_and_metadata_mutation() {
    for source in [
        "int main() { int[2] a = {1,2}; const int[]& s = &a; s[0] = 3; return 0; }",
        "int main() { int[2] a = {1,2}; int[]& s = &a; a[0] = 3; print(s[0]); return 0; }",
        "int main() { int[2] a = {1,2}; int[]& s = &a; s.length = 20; return 0; }",
        "int main() { int[2] a = {1,2}; int[]& s = &a; int[2] b = {3,4}; s = b; return 0; }",
        "const int[]& bad() { int[2] a = {1,2}; return &a; } int main() { return 0; }",
        "int main() { int[] a = {1,2}; return 0; }",
        "int main() { int[2] a = {1,2}; const char[]& s = &a; return 0; }",
        "int main() { int[2] a = {1,2}; const int[]& s = &a; int[]& t = &s; return 0; }",
    ] {
        assert!(crust::compile(source).is_err(), "accepted {source}");
    }
}

#[test]
fn slice_indexing_reports_dynamic_bounds_failures() {
    let output = common::tracked_run(
        "int read(const int[]& values, int i) { return values[i]; } int main() { int[2] a = {1,2}; print(read(&a, 2)); return 0; }",
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("out of bounds"));
}

#[test]
fn raw_slice_construction_requires_unsafe_and_checks_null_storage() {
    run(
        "int main() { int* p = 0; unsafe { const int[]& empty = slice(p, 0); print(empty.length); } return 0; }",
        "0\n",
    );
    assert!(
        crust::compile("int main() { int* p = 0; const int[]& empty = slice(p, 0); return 0; }")
            .is_err()
    );
    let output = common::tracked_run(
        "int main() { int* p = 0; unsafe { const int[]& invalid = slice(p, 1); print(invalid.length); } return 0; }",
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid slice storage"));
}

#[test]
fn slices_preserve_nested_array_shapes_and_borrowed_pointer_reads() {
    run(
        r#"
        int read(const int[][2]& rows) { return rows[1][0]; }
        int main() {
            int[2][2] rows = { {1,2}, {3,4} };
            print(read(&rows));
            int* pointer = &rows[0][0];
            int*& alias = &pointer;
            unsafe { const int[]& values = slice(alias, 2); print(values[1]); }
            return 0;
        }
    "#,
        "3\n2\n",
    );
}
