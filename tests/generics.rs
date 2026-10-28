mod common;
use common::compiler as crust;

fn run(source: &str, stdout: &str, drops: &str) {
    let output = common::tracked_run_sizes(source);
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
fn generic_functions_keep_types_constants_and_conversions_per_instantiation() {
    run(
        r#"
        T identity<T>(T value) { return value; }
        int64_t widen<T>(T value) { return value; }
        T factorial<T>(T n) {
            if (n <= 1) { return 1; }
            return n * factorial<T>(n - 1);
        }
        int main() {
            print(identity<int>(42));
            print(identity<uint64_t>(18446744073709551615));
            print(widen<int>(-1)); print(widen<uint64_t>(42));
            print(factorial<int>(5)); print(factorial<int64_t>(6));
            return 0;
        }
    "#,
        "42\n18446744073709551615\n-1\n42\n120\n720\n",
        "",
    );
}

#[test]
fn generic_structs_support_aliases_methods_and_nested_closing_angles() {
    run(
        r#"
        typedef Pair<int, int> IntPair;
        struct Pair<T, U> {
            T first;
            U second;
            static Self create(T first, U second) { return (Self){ .first = first, .second = second }; }
            T get_first() const { return this.first; }
        };
        int main() {
            IntPair inner = IntPair::create(1, 2);
            Pair<Pair<int, int>, int64_t> outer = Pair<IntPair, int64_t>::create(inner, 3);
            IntPair copy = outer.get_first();
            print(copy.first); print(outer.second);
            Pair<int64_t, char> other = Pair<int64_t, char>::create(9223372036854775807, 'a');
            print(other.get_first()); print(other.second);
            return 0;
        }
    "#,
        "1\n3\n9223372036854775807\n97\n",
        "",
    );
}

#[test]
fn nested_type_arguments_and_generic_type_and_value_names_remain_distinct() {
    run(
        r#"
        struct Wrap<T> { T value; static Self create(T value) { return (Self){ .value = value }; } };
        T Wrap<T>(T value) { return value; }
        typedef int Count;
        int main() {
            Wrap<Wrap<Wrap<int>>> nested = { .value = { .value = { .value = 42 } } };
            print(nested.value.value.value);
            Wrap<Count> first = Wrap<int>::create(3);
            Wrap<int> second = first;
            print(Wrap<int>(second.value));
            Wrap<int>(second.value);
            Wrap<int>[2] array = { { .value = 5 }, { .value = 6 } };
            print(array[1].value);
            return 0;
        }
    "#,
        "42\n3\n6\n",
        "",
    );
}

#[test]
fn generics_preserve_ownership_borrowed_getters_and_automatic_field_cleanup() {
    run(
        r#"
        struct Holder<T> {
            private T value;
            static Self create(T value) { return (Self){ .value = move(value) }; }
            const T& get() const { return &const this.value; }
        };
        T transfer<T>(T value) { return move(value); }
        int main() {
            Holder<Box<int>> first = Holder<Box<int>>::create(Box<int>::create(42));
            Holder<Box<int>> second = transfer<Holder<Box<int>>>(move(first));
            print(second.get().get());
            Box<int> third = transfer<Box<int>>(Box<int>::create(7)); print(third.get());
            return 0;
        }
    "#,
        "42\n7\n",
        "drop:bytes:4\ndrop:bytes:4\n",
    );
}

#[test]
fn generic_functions_can_supply_c_callbacks_at_multiple_widths() {
    run(
        r#"
        typedef int (*Narrow)(int);
        typedef int64_t (*Wide)(int64_t);
        T identity<T>(T value) { return value; }
        int main() {
            Narrow narrow = identity<int>;
            Wide wide = &identity<int64_t>;
            unsafe { print(narrow(4)); print(wide(9223372036854775807)); }
            return 0;
        }
    "#,
        "4\n9223372036854775807\n",
        "",
    );
}

#[test]
fn instantiation_scopes_do_not_capture_callers_type_or_value_names() {
    run(
        r#"
        typedef int T;
        T global_alias<TUnused>(T value) { return value; }
        U outer<U>(U value) { return global_alias<U>(42); }
        size_t size<T>() { return sizeof(T); }
        int main() {
            int T = 100;
            print(size<int64_t>()); print(size<char>());
            print(outer<uint64_t>(1)); print(T);
            return 0;
        }
    "#,
        "8\n1\n42\n100\n",
        "",
    );
}

#[test]
fn rejects_invalid_arguments_duplicate_parameters_and_infinite_instantiation() {
    for source in [
        "struct Pair<T, T> { T value; }; int main() { return 0; }",
        "struct Pair<Self> { int value; }; int main() { return 0; }",
        "struct Pair<T> { T value; }; int main() { Pair<int, bool> value = { .value = 1 }; return 0; }",
        "struct Pair<T> { T value; }; int main() { Pair value = { .value = 1 }; return 0; }",
        "T identity<T>(T value) { return value; } int main() { identity<int, bool>(1); return 0; }",
        "T identity<T>(T value) { return value; } int main() { T value = 1; return 0; }",
        "struct Grow<T> { Grow<Grow<T>>* value; }; int main() { Grow<int>* p = 0; return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}

#[test]
fn c_backed_generic_buffers_initialize_borrow_move_and_free_each_element_width() {
    run(
        include_str!("../examples/generic_buffer.cr"),
        "10\n20\n98\n3\n4\n",
        "drop:bytes:16\ndrop:bytes:3\ndrop:bytes:12\n",
    );
    let definitions = include_str!("../examples/generic_buffer.cr")
        .split("int main()")
        .next()
        .unwrap();
    run(
        &format!(
            "{definitions} int main() {{ Buffer<int64_t> first = Buffer<int64_t>::create(2, 9223372036854775807); Buffer<int64_t> second = move(first); print(second.at(1)); Buffer<char> empty = Buffer<char>::create(0, 'a'); print(empty.count()); return 0; }}"
        ),
        "9223372036854775807\n0\n",
        "drop:bytes:16\n",
    );
    run(
        &format!("{definitions} int main() {{ Buffer<int>::create(1, 3); return 0; }}"),
        "",
        "drop:bytes:4\n",
    );
    for body in [
        "Buffer<int> value = Buffer<int>::create(2, 0); const int& view = value.at(0); Buffer<int> next = move(value);",
        "Buffer<int> value = { .data = 0, .length = 2 };",
        "Buffer<Box<int>> value = Buffer<Box<int>>::create(2, Box<int>::create(1));",
    ] {
        assert!(crust::check(&format!("{definitions} int main() {{ {body} return 0; }}")).is_err());
    }
}
