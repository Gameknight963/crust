mod common;
use common::compiler as crust;

fn run(source: &str, expected: &str) {
    run_with_drops(source, expected, "");
}

fn run_with_drops(source: &str, expected: &str, drops: &str) {
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
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        drops
    );
}

fn fails(source: &str, expected: &str) {
    let output = common::tracked_run(source);
    assert!(!output.status.success(), "should fail: {source}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(expected),
        "{source}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn runs_fixed_width_integer_boundaries_and_unsigned_wraparound() {
    run(
        include_str!("../examples/integers.cr"),
        "-128\n65535\n-9223372036854775808\n18446744073709551615\n0\n",
    );
}

#[test]
fn integer_bases_and_character_literals_work_in_expressions_and_contexts() {
    run(
        r#"
        uint8_t letter(uint8_t c) { return c; }
        int main() {
            uint64_t bits = 0xffffffffffffffff;
            print(bits); print(077); print(0b1010 + 0X10);
            print(letter('A')); print('\n'); print('\0');
            print('\xff'); print('\377'); print('z' - 'a');
            uint8_t high = '\x80'; print(high);
            print(sizeof('a'));
            return 0;
        }
        "#,
        "18446744073709551615\n63\n26\n65\n10\n0\n255\n255\n25\n128\n4\n",
    );
    for source in [
        r"int main() { int8_t a = '\xff'; return 0; }",
        "int main() { int8_t a = 0x80; return 0; }",
    ] {
        assert!(crust::check(source).is_err());
    }
}

#[test]
fn arithmetic_promotions_and_mixed_integer_types_preserve_values() {
    run(
        r#"
        int main() {
            int8_t a = 127;
            uint8_t b = 255;
            print(a + 1); print(b + 1);
            uint32_t c = 4294967295;
            int64_t d = -1;
            print(c + d);
            uint64_t e = 18446744073709551615;
            print(e / 3);
            e += 1; print(e);
            e -= 1; print(e);
            uint32_t u = 1;
            print(-u);
            print(c > a);
            return 0;
        }
    "#,
        "128\n256\n4294967294\n6148914691236517205\n0\n18446744073709551615\n4294967295\n1\n",
    );
}

#[test]
fn checked_conversions_work_in_arguments_returns_fields_and_assignments() {
    run(
        r#"
        typedef uint8_t Byte;
        struct Packet { Byte tag; int64_t count; };
        Byte narrow(int64_t n) { return n; }
        void increment(uint64_t& n) { n += 1; }
        uint64_t read(const uint64_t& n) { return n; }
        int main() {
            int64_t n = 42;
            Packet p = { .tag = narrow(n), .count = n };
            print(p.tag); print(p.count);
            uint64_t wide = n;
            increment(&wide);
            print(read(&const wide));
            bool flag = wide; print(flag);
            int8_t small = flag; print(small);
            return 0;
        }
    "#,
        "42\n42\n43\n1\n1\n",
    );
}

#[test]
fn pointer_sized_aliases_are_consistent_with_fixed_width_types() {
    let unsigned = if usize::BITS == 64 {
        "uint64_t"
    } else {
        "uint32_t"
    };
    let signed = if usize::BITS == 64 {
        "int64_t"
    } else {
        "int32_t"
    };
    run(
        &format!(
            r#"
        void increment({unsigned}& n) {{ n += 1; }}
        int main() {{
            size_t length = 41;
            increment(&length);
            uintptr_t address = length;
            ptrdiff_t difference = -1;
            {signed} copy = difference;
            intptr_t integer = copy;
            print(address); print(integer);
            return 0;
        }}
    "#
        ),
        "42\n-1\n",
    );
}

#[test]
fn small_integer_compound_assignments_check_narrowing() {
    run(
        "int main() { int8_t n = 12; n *= 2; n += 3; print(n); return 0; }",
        "27\n",
    );
    fails(
        "int main() { int8_t n = 127; n += 1; return 0; }",
        "conversion out of range",
    );
    fails(
        "int main() { uint8_t n = 255; n += 1; return 0; }",
        "conversion out of range",
    );
}

#[test]
fn dynamic_numeric_conversions_reject_out_of_range_values() {
    for body in [
        "int n = 128; int8_t small = n;",
        "int n = -1; uint8_t small = n;",
        "uint32_t n = 4294967295; int small = n;",
        "uint64_t n = 18446744073709551615; int64_t small = n;",
        "int64_t n = 4294967296; uint32_t small = n;",
        "int64_t n = -1; uint64_t small = n;",
        "int n = -1; uint32_t u = 1; print(n + u);",
    ] {
        fails(
            &format!("int main() {{ {body} return 0; }}"),
            "conversion out of range",
        );
    }
}

#[test]
fn signed_64_bit_multiplication_accepts_boundary_products() {
    let pairs = [
        (i64::MIN, 0),
        (i64::MIN, 1),
        (i64::MAX, 1),
        (-1, i64::MAX),
        (-3037000499, -3037000499),
        (3037000499, 3037000499),
        (i64::MIN / 2, 2),
        (-2, i64::MAX / 2),
    ];
    let mut body = String::new();
    let mut expected = String::new();
    for (index, (a, b)) in pairs.into_iter().enumerate() {
        body.push_str(&format!(
            "int64_t a{index} = {a}; int64_t b{index} = {b}; print(a{index} * b{index});"
        ));
        expected.push_str(&format!("{}\n", i128::from(a) * i128::from(b)));
    }
    run(&format!("int main() {{ {body} return 0; }}"), &expected);
}

#[test]
fn signed_64_bit_arithmetic_rejects_overflow_before_c_operations() {
    for (a, operator, b) in [
        (i64::MAX, "+", 1),
        (i64::MIN, "+", -1),
        (i64::MIN, "-", 1),
        (i64::MAX, "-", -1),
        (i64::MIN, "*", -1),
        (-1, "*", i64::MIN),
        (i64::MAX, "*", 2),
        (i64::MIN, "*", 2),
        (-3037000500, "*", -3037000500),
        (3037000500, "*", -3037000500),
        (i64::MIN, "/", -1),
        (i64::MIN, "%", -1),
    ] {
        fails(
            &format!(
                "int main() {{ int64_t a = {a}; int64_t b = {b}; print(a {operator} b); return 0; }}"
            ),
            "integer overflow",
        );
    }
    fails(
        "int main() { int64_t n = -9223372036854775808; print(-n); return 0; }",
        "integer overflow",
    );
}

#[test]
fn wide_integer_division_and_remainder_check_zero() {
    for ty in ["int64_t", "uint32_t", "uint64_t"] {
        for (op, reason) in [("/", "division by zero"), ("%", "remainder by zero")] {
            fails(
                &format!("int main() {{ {ty} n = 1; print(n {op} 0); return 0; }}"),
                reason,
            );
        }
    }
}

#[test]
fn c_style_casts_check_ranges_and_support_bitwise_complement() {
    run_with_drops(
        r#"
        int mark() { print(999); return 42; }
        int main() {
            int64_t wide = 42;
            print((int8_t)wide);
            print((uint64_t)1 + 4294967295);
            print(~(uint64_t)0);
            int8_t negative = -128;
            print(~negative);
            (void)mark();
            Box<int> owner = Box<int>::create(7);
            (void)move(owner);
            int n = 1;
            (void)&n;
            n = 2;
            print(n);
            return 0;
        }
    "#,
        "42\n4294967296\n18446744073709551615\n127\n999\n2\n",
        "drop:7\n",
    );
}

#[test]
fn sizeof_and_alignof_query_types_without_running_expressions_or_moves() {
    run_with_drops(
        r#"
        struct Packet { uint8_t tag; uint64_t value; };
        int mark() { print(999); return 1; }
        int main() {
            int n = sizeof(n);
            print(n);
            print(sizeof(uint64_t));
            print(alignof(uint8_t));
            print(sizeof(Packet) >= 9);
            print(alignof(Packet) >= _Alignof(uint64_t));
            print(sizeof(mark()));
            print(sizeof(Box<int>::create(1)) == sizeof(Box<int>));
            Box<int> owner = Box<int>::create(7);
            print(sizeof(move(owner)) == sizeof(Box<int>));
            print(owner.get());
            Box<int> other = move(owner);
            print(sizeof(owner) == sizeof(other));
            print(sizeof(const int&) == sizeof(Box<int>));
            return 0;
        }
    "#,
        "4\n8\n1\n1\n1\n4\n1\n1\n7\n1\n1\n",
        "drop:7\n",
    );
}

#[test]
fn casts_and_sizeof_distinguish_type_names_from_shadowing_values() {
    run(
        r#"
        typedef int64_t Count;
        int query(int Count) { return sizeof(Count); }
        int cast_after_loop() {
            for (int Count = 0; Count < 1; Count += 1) { }
            return (Count)7;
        }
        int main() {
            int Count = 3;
            print((Count) + 1);
            print(sizeof(Count));
            print(query(0));
            print(cast_after_loop());
            return 0;
        }
    "#,
        "4\n4\n4\n7\n",
    );
}

#[test]
fn explicit_casts_retain_checked_conversion_behavior() {
    fails(
        "int main() { int n = 128; print((int8_t)n); return 0; }",
        "conversion out of range",
    );
    fails(
        "int main() { int n = -1; print((uint64_t)n); return 0; }",
        "conversion out of range",
    );
    for source in [
        "int main() { print((int8_t)128); return 0; }",
        "int main() { print(sizeof(void)); return 0; }",
        "int main() { print(alignof(void)); return 0; }",
        "int main() { print(sizeof(print(1))); return 0; }",
        "int main() { print(sizeof(missing)); return 0; }",
        "int main() { Box<int> a = Box<int>::create(1); (void)a; return 0; }",
        "struct A { int n; }; int main() { A a = { .n = 1 }; (A)a; return 0; }",
        "int main() { int n = 1; (int&)n; return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}

#[test]
fn contextual_literals_and_borrows_keep_exact_ranges_and_types() {
    for source in [
        "int main() { int8_t n = 128; return 0; }",
        "int main() { int8_t n = -129; return 0; }",
        "int main() { uint8_t n = -1; return 0; }",
        "int main() { uint64_t n = -1; return 0; }",
        "int main() { int64_t n = 9223372036854775808; return 0; }",
        "int main() { print(-18446744073709551615); return 0; }",
        "void f(int64_t& p) {} int main() { int n = 1; f(&n); return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}

#[test]
fn sizeof_and_parentheses_distinguish_scoped_values_from_generic_types() {
    run(
        r#"
enum class Code : uint8_t { Value };
struct Pair<T> {
    T value;
    static int make() { print(99); return 7; }
};
int main() {
    print(sizeof(Pair<Pair<int>>::make()));
    print(sizeof(Code::Value));
    print((Pair<int>::make()));
    return 0;
}
"#,
        "4\n1\n99\n7\n",
    );
}
