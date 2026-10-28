mod common;
use common::compiler as crust;

#[test]
fn constant_lengths_work_in_aliases_fields_and_nested_generic_declarations() {
    run(
        r#"
        typedef int Row[(Columns + 1)];
        typedef int[1 << 2] Block;
        enum Sizes { Columns = 1 << 1 };
        struct Holder<T> { T value; };
        struct Grid { int cells[Columns + 1]; };
        int main() {
            Row row = { 1, 2, 3 };
            Block block = { 4, 5, 6, 7 };
            Grid grid = { .cells = row };
            Holder<Holder<int[8 >> 2]>> nested = { .value = { .value = { 8, 9 } } };
            Holder<int> copies[1 + 1] = { { .value = 10 }, { .value = 11 } };
            Holder<int[(1 < 2) + 1]> comparisons = { .value = { 12, 13 } };
            print(grid.cells[2]); print(block[3]); print(nested.value.value[1]);
            print(copies[1].value); print(comparisons.value[1]);
            return 0;
        }
    "#,
        "3\n7\n9\n11\n13\n",
        "",
    );
}

#[test]
fn computed_lengths_preserve_owning_array_cleanup() {
    run(
        "int main() { Box<int> owners[1 << 1] = { Box<int>::create(1), Box<int>::create(2) }; print(owners[1].get()); return 0; }",
        "2\n",
        "drop:2\ndrop:1\n",
    );
}

#[test]
fn array_constants_use_global_enum_context_and_respect_local_shadowing() {
    let source = r#"
        typedef uint8_t T;
        int dimensions<T>(int Count) {
            int values[Sizes::Count] = { 1, 2 };
            return sizeof(values) == 2 * sizeof(int);
        }
        int main() { print(dimensions<int>(99)); return 0; }
        enum Sizes : T { Count = 2 };
    "#;
    run(source, "1\n", "");
    let shadowed = "enum Sizes { Count = 2 }; int main() { int Count = 2; int values[Count] = { 1, 2 }; return 0; }";
    assert!(
        crust::check(shadowed)
            .unwrap_err()
            .message
            .contains("local value")
    );
}

#[test]
fn array_length_expressions_reject_runtime_values_and_invalid_constant_arithmetic() {
    for expression in ["0", "-1", "1 / 0", "1 << 32", "2147483647 + 1", "length()"] {
        let source = format!(
            "int length() {{ return 2; }} int main() {{ int values[{expression}] = {{ 1 }}; return 0; }}"
        );
        assert!(crust::check(&source).is_err(), "{expression}");
    }
}

#[test]
fn imported_array_constants_keep_definition_and_reference_locations() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/virtual-array-constants");
    let entry = "import \"sizes.cr\"; typedef int Row[Columns + 1]; int main() { Row values = { 1, 2, 3 }; return values[2] - 3; }";
    let helper = "enum Sizes { Columns = 2 };";
    let sources = crust::source::SourceSet::load_with(&root.join("main.cr"), |path| {
        return Ok(if path.ends_with("main.cr") {
            entry.to_owned()
        } else {
            helper.to_owned()
        });
    })
    .unwrap();
    let analysis = sources.analyze().unwrap();
    let offset = entry.find("Columns +").unwrap();
    let definition = analysis.definitions_at(0, offset);
    assert_eq!(definition.len(), 1);
    assert_eq!(definition[0].location.file, 1);
    assert_eq!(analysis.references_at(0, offset, true).len(), 2);
    let output = common::tracked_generated_c(&sources.compile().unwrap(), false);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
}

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
fn runs_array_example_and_destroys_elements_in_reverse_order() {
    common::assert_example(
        "examples/arrays.cr",
        "62\n4\n",
        "drop:5\ndrop:6\ndrop:4\n",
        false,
    );
}

#[test]
fn nested_arrays_fields_and_aliases_have_value_semantics() {
    run(
        r#"
        typedef int Row[3];
        typedef Row Matrix[2];
        typedef int[3] SameRow;
        struct Grid { Matrix values; int flags[2]; };
        Row copy(Row row) { row[0] = 99; return row; }
        Row create() { return (Row){ 7, 8, 9 }; }
        int main() {
            int direct[2][3] = { { 1, 2, 3 }, { 4, 5, 6 } };
            Matrix matrix = direct;
            Row literal = (int[3]){ 1, 2, 3 };
            print(literal[2]);
            Grid grid = { .flags = { 0, 1 }, .values = matrix };
            SameRow row = copy(grid.values[0]);
            print(row[0]); print(grid.values[0][0]);
            grid.values[1] = create();
            print(grid.values[1][2]); print(grid.flags[1]);
            matrix = { { 10, 20, 30 }, { 40, 50, 60 } };
            print(matrix[1][2]);
            print(sizeof(direct)); print(alignof(Matrix));
            print(sizeof(int[2][3]));
            return 0;
        }
    "#,
        "3\n99\n1\n9\n1\n60\n24\n4\n24\n",
        "",
    );
}

#[test]
fn array_borrows_support_element_access_and_nested_reborrowing() {
    run(
        r#"
        typedef int Row[2];
        void increment(int& value) { value += 1; }
        int read(const int& value) { return value; }
        void update(Row& row) { increment(&(row)[1]); }
        int sum(const Row& row) { return read(&const (row)[0]) + (row)[1]; }
        int main() {
            Row row = { 3, 4 };
            update(&row); print(sum(&const row));
            return 0;
        }
    "#,
        "8\n",
        "",
    );
}

#[test]
fn owning_arrays_move_through_calls_returns_replacement_and_discard() {
    run(
        r#"
        typedef Box<int> Owners[2];
        Owners create(int n) { return (Owners){ Box<int>::create(n), Box<int>::create(n + 1) }; }
        Owners transfer(Owners owners) { return move(owners); }
        int main() {
            Owners first = create(1);
            Owners second = transfer(move(first));
            first = create(3);
            second[0] = Box<int>::create(5);
            second = move(first);
            create(6);
            return 0;
        }
    "#,
        "",
        "drop:1\ndrop:2\ndrop:5\ndrop:7\ndrop:6\ndrop:4\ndrop:3\n",
    );
}

#[test]
fn array_cleanup_respects_destructors_nested_fields_and_loop_control() {
    run(
        r#"
        struct Item {
            int id;
            Box<int> owners[2];
            ~Item() { print(this.id); }
        };
        Item create(int id) { return (Item){ .id = id, .owners = { Box<int>::create(id), Box<int>::create(id + 1) } }; }
        int main() {
            for (int i = 0; i < 2; i += 1) {
                Item items[2] = { create(i * 10), create(i * 10 + 2) };
                if (i == 0) { continue; }
                break;
            }
            Item final[1] = { create(20) };
            return 0;
        }
    "#,
        "2\n0\n12\n10\n20\n",
        "drop:3\ndrop:2\ndrop:1\ndrop:0\ndrop:13\ndrop:12\ndrop:11\ndrop:10\ndrop:21\ndrop:20\n",
    );
}

#[test]
fn indices_evaluate_once_with_defined_assignment_and_compound_order() {
    run(
        r#"
        int mark(int n) { print(n); return n; }
        int main() {
            int values[3] = { mark(0), mark(1), mark(2) };
            values[mark(1)] = mark(2);
            values[mark(0)] += mark(2);
            print(values[mark(0)]);
            print(sizeof(values[mark(1)]));
            return 0;
        }
    "#,
        "0\n1\n2\n2\n1\n0\n2\n0\n2\n4\n",
        "",
    );
}

#[test]
fn dynamic_indices_reject_negative_large_and_unsigned_out_of_bounds_values() {
    for index in ["-1", "2", "18446744073709551615"] {
        let source = format!(
            "int main() {{ int values[2] = {{ 1, 2 }}; {} index = {index}; print(values[index]); return 0; }}",
            if index.starts_with('-') {
                "int64_t"
            } else {
                "uint64_t"
            }
        );
        let output = common::tracked_run(&source);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("array index out of bounds"));
    }
}

#[test]
fn rejects_invalid_lengths_initializers_and_incompatible_array_types() {
    for source in [
        "int main() { int values[0] = {}; return 0; }",
        "int main() { int n = 2; int values[n] = {1, 2}; return 0; }",
        "int main() { void values[2] = {1, 2}; return 0; }",
        "int main() { int& values[2] = {1, 2}; return 0; }",
        "int main() { int values[2] = {1}; return 0; }",
        "int main() { int values[2] = {1, 2, 3}; return 0; }",
        "int main() { int values[2] = {1, 2}; print(values[2]); return 0; }",
        "int main() { int values[2] = {1, 2}; print(values[-1]); return 0; }",
        "int main() { int n = 0; print(n[0]); return 0; }",
        "int main() { int values[2] = {1, 2}; print(values[Box<int>::create(0)]); return 0; }",
        "int main() { int a[2] = {1, 2}; int b[3] = a; return 0; }",
        "int main() { int a[2] = {1, 2}; uint32_t b[2] = a; return 0; }",
        "struct Loop { Loop elements[1]; }; int main() { return 0; }",
        "typedef Next First[2]; typedef First Next; int main() { return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}

#[test]
fn arrays_preserve_parent_ownership_and_borrow_permissions() {
    for source in [
        "int main() { Box<int> a[1] = {Box<int>::create(1)}; Box<int> b = a[0]; return 0; }",
        "int main() { Box<int> a[1] = {Box<int>::create(1)}; Box<int> b = move(a[0]); return 0; }",
        "int main() { Box<int> a[1] = {Box<int>::create(1)}; Box<int> b[1] = a; return 0; }",
        "int main() { Box<int> a[1] = {Box<int>::create(1)}; Box<int> b[1] = move(a); print(a[0].get()); return 0; }",
        "typedef int Row[2]; void f(const Row& a) { (a)[0] = 3; } int main() { return 0; }",
        "typedef int Row[2]; void f(const Row& a) { (a)[0] += 3; } int main() { return 0; }",
        "typedef int Row[2]; void f(int& a, int& b) {} int main() { Row a = {1, 2}; f(&a[0], &a[1]); return 0; }",
        "typedef Box<int> Owners[1]; int consume(Owners owners) { return 0; } int main() { Owners a = {Box<int>::create(1)}; print(a[consume(move(a))].get()); return 0; }",
        "typedef int Row[2]; int change(Row& a) { (a)[0] = 2; return 0; } int main() { Row a = {1, 2}; print(a[change(&a)]); return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}
