mod common;

fn run(source: &str, expected: &str) {
    let output = common::tracked_run(source);
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
fn callbacks_use_function_addresses_pointer_calls_and_fields() {
    run(include_str!("../examples/callbacks.cr"), "42\n42\n42\n");
}

#[test]
fn static_methods_supply_callbacks_through_types_aliases_and_self() {
    run(
        r#"
        typedef int (*Callback)(int);
        struct Operations {
            int unused;
            static int increment(int value) { return value + 1; }
            static Callback get() { return Self::increment; }
        };
        typedef Operations Alias;
        int main() {
            Callback first = Alias::increment;
            Callback second = &Operations::increment;
            Callback third = Operations::get();
            unsafe { print(first(1)); print(second(2)); print(third(3)); }
            return 0;
        }
    "#,
        "2\n3\n4\n",
    );
}

#[test]
fn function_typedefs_pointer_aliases_arrays_and_nested_signatures_work() {
    run(
        r#"
        typedef int Operation(int value);
        typedef Operation* Callback;
        typedef Callback Factory();
        typedef int (*Traditional)(int);
        int increment(int n) { return n + 1; }
        int twice(int n) { return n * 2; }
        Callback choose() { return increment; }
        int main() {
            Callback array[2] = { &increment, twice };
            Factory* factory = choose;
            Traditional copy = array[0];
            Callback empty = 0;
            print(!empty);
            unsafe {
                print(array[0](1)); print(array[1](2));
                print(factory()(5));
                Callback decayed = *copy;
                print(decayed(6));
                Callback* slot = &copy;
                print((**slot)(7));
            }
            print(sizeof(Callback) == sizeof(void*));
            return 0;
        }
    "#,
        "1\n2\n4\n6\n7\n8\n1\n",
    );
}

#[test]
fn callbacks_evaluate_the_callee_before_arguments_and_only_once() {
    run(
        r#"
        typedef int (*Callback)(int, int);
        int combine(int a, int b) { return a * 10 + b; }
        Callback choose() { print(1); return combine; }
        int mark(int value) { print(value); return value; }
        int main() {
            unsafe { print(choose()(mark(2), mark(3))); }
            return 0;
        }
    "#,
        "1\n2\n3\n23\n",
    );
}

#[test]
fn callback_returns_can_supply_raw_pointer_assignment_destinations() {
    run(
        r#"
        typedef int* (*Pick)(int*);
        int* identity(int* p) { return p; }
        struct Holder { Pick pick; };
        int main() {
            int value = 1;
            Pick callback = identity;
            Holder holder = { .pick = callback };
            unsafe {
                *callback(&value) = 2;
                *holder.pick(&value) += 3;
            }
            print(value);
            return 0;
        }
    "#,
        "5\n",
    );
}

#[test]
fn function_addresses_are_safe_and_indirect_calls_require_unsafe() {
    crust::check("typedef int (*Callback)(int); int f(int n) { return n; } int main() { Callback a = f; Callback b = &f; print(a == b); return 0; }").unwrap();
    for source in [
        "typedef int (*Callback)(int); int f(int n) { return n; } int main() { Callback a = f; print(a(1)); return 0; }",
        "typedef int (*Callback)(int); int f(int n) { return n; } int main() { Callback a = f; print((*a)(1)); return 0; }",
    ] {
        assert!(
            crust::check(source)
                .unwrap_err()
                .message
                .contains("unsafe block")
        );
    }
}

#[test]
fn rejects_incompatible_signatures_non_c_values_and_invalid_function_objects() {
    for source in [
        "typedef int (*Callback)(int); int64_t f(int n) { return n; } int main() { Callback a = f; return 0; }",
        "typedef int (*Callback)(int); int f(int n) { return n; } int main() { Callback a = f; unsafe { a(); } return 0; }",
        "typedef int (*Callback)(int); int f(int n) { return n; } int main() { Callback a = f; unsafe { a(box_int(1)); } return 0; }",
        "typedef BoxInt (*Callback)(int); int main() { return 0; }",
        "typedef int (*Callback)(const int&); int main() { return 0; }",
        "typedef int (*Callback)(void value); int main() { return 0; }",
        "typedef int Function(int); int main() { Function value = 0; return 0; }",
        "typedef int Function(int); int main() { print(sizeof(Function)); return 0; }",
        "typedef int Function(int); int main() { const Function* value = 0; return 0; }",
        "int main() { (void)&box_int; return 0; }",
        "typedef int Function(int); int main() { Function* p = 0; unsafe { p += 1; } return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}
