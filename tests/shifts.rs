mod common;

fn run(source: &str, expected: &str) {
    let output = common::tracked_run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        expected
    );
}

#[test]
fn shifts_use_c_precedence_left_associativity_and_independent_integer_promotions() {
    run(
        r#"
        int main() {
            print(1 << 2 + 1);
            print(32 >> 1 >> 2);
            print(1 << 2 < 5);
            print(8 >> 1 + 1);
            print(sizeof((uint8_t)1 << (uint64_t)1) == sizeof(int));
            print(sizeof((uint32_t)1 << (int64_t)1) == sizeof(uint32_t));
            print(sizeof((long)1 << (long long)1) == sizeof(long));
            print((uint8_t)128 << 1);
            print(true << 2);
            return 0;
        }
    "#,
        "8\n4\n1\n2\n1\n1\n1\n256\n4\n",
    );
}

#[test]
fn signed_shifts_preserve_sign_and_check_multiplication_while_unsigned_left_wraps() {
    run(
        r#"
        int main() {
            print(-3 >> 1); print(-1 >> 31); print(-1 << 31);
            print(-2147483648 >> 31); print(-2147483648 << 0);
            print((int64_t)-1 << 63); print((int64_t)-3 >> 1);
            print((int64_t)-9223372036854775808 >> 63);
            print((uint32_t)4294967295 << 1);
            print((uint64_t)1 << 63);
            print((uint64_t)18446744073709551615 >> 63);
            print((uint64_t)18446744073709551615 << 1);
            return 0;
        }
    "#,
        "-2\n-1\n-2147483648\n-1\n-2147483648\n-9223372036854775808\n-2\n-1\n4294967294\n9223372036854775808\n1\n18446744073709551614\n",
    );
}

#[test]
fn compound_shifts_capture_destinations_once_and_check_narrowing() {
    run(
        r#"
        int index(int& calls) { calls += 1; print(calls); return 0; }
        int amount(int& calls) { calls += 1; print(calls); return 2; }
        int main() {
            int calls = 0;
            int values[1] = { 3 };
            values[index(&calls)] <<= amount(&calls);
            print(values[0]); print(calls);
            uint8_t byte = 128; byte >>= (uint64_t)1; print(byte);
            int64_t wide = -3; wide >>= 1; print(wide);
            uint32_t wrapped = 4294967295; wrapped <<= 1; print(wrapped);
            return 0;
        }
    "#,
        "1\n2\n12\n2\n64\n-2\n4294967294\n",
    );
    let output = common::tracked_run("int main() { uint8_t value = 128; value <<= 1; return 0; }");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("integer conversion out of range"));
}

#[test]
fn invalid_counts_and_signed_overflow_fail_before_any_undefined_c_shift() {
    for expression in [
        "1 << -1",
        "1 >> -1",
        "1 << 32",
        "1 >> (uint64_t)18446744073709551615",
        "(uint64_t)1 >> 64",
    ] {
        let source = format!("int main() {{ print({expression}); return 0; }}");
        let output = common::tracked_run(&source);
        assert!(!output.status.success(), "{expression}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("shift count out of range"),
            "{expression}"
        );
    }
    for expression in [
        "1 << 31",
        "-2 << 31",
        "(int64_t)1 << 63",
        "(int64_t)-2 << 63",
    ] {
        let source = format!("int main() {{ print({expression}); return 0; }}");
        let output = common::tracked_run(&source);
        assert!(!output.status.success(), "{expression}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("integer overflow"),
            "{expression}"
        );
    }
}

#[test]
fn right_shifts_coexist_with_nested_generic_closing_angles_and_unevaluated_queries() {
    run(
        r#"
        struct Holder<T> { T value; };
        typedef Holder<Holder<int>> Nested;
        T identity<T>(T value) { return value; }
        int main() {
            print(identity<int>(64) >> 2);
            print(sizeof(Nested) == sizeof(int));
            print(sizeof(1 << 100) == sizeof(int));
            return 0;
        }
    "#,
        "16\n1\n1\n",
    );
}

#[test]
fn shift_operands_require_integers_and_mutable_compound_destinations() {
    for source in [
        "int main() { int* pointer = 0; unsafe { print(pointer << 1); } return 0; }",
        "int main() { int value = 1; const int& shared = &const value; shared <<= 1; return 0; }",
        "int main() { BoxInt value = box_int(1); print(value >> 1); return 0; }",
        "int main() { int* pointer = 0; unsafe { pointer >>= 1; } return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "{source}");
    }
}

#[test]
fn compiled_shift_results_match_every_valid_count_at_signed_and_unsigned_boundaries() {
    let mut expected = String::new();
    for value in [i64::MIN, i64::MIN + 1, -3, -1, 0, 1, i64::MAX] {
        for count in 0..64 {
            expected.push_str(&format!("{}\n", value >> count));
        }
    }
    for count in 0..64 {
        expected.push_str(&format!("{}\n{}\n", u64::MAX << count, -1_i64 << count));
    }
    for count in 0..32 {
        expected.push_str(&format!("{}\n{}\n", u32::MAX << count, -1_i32 << count));
    }
    run(
        r#"
        int main() {
            int64_t values[7] = { -9223372036854775808, -9223372036854775807, -3, -1, 0, 1, 9223372036854775807 };
            for (int i = 0; i < 7; i += 1) {
                for (int count = 0; count < 64; count += 1) { print(values[i] >> count); }
            }
            for (int count = 0; count < 64; count += 1) {
                print((uint64_t)18446744073709551615 << count);
                print((int64_t)-1 << count);
            }
            for (int count = 0; count < 32; count += 1) {
                print((uint32_t)4294967295 << count);
                print(-1 << count);
            }
            return 0;
        }
    "#,
        &expected,
    );
}
