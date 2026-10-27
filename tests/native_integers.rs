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
    assert!(output.stderr.is_empty());
}

#[test]
fn native_integer_spellings_preserve_c_library_signatures_and_checked_arithmetic() {
    run(
        include_str!("../examples/native_integers.cr"),
        "-128\n65535\n0\n2147483647\n9223372036854775807\n2147483647\n",
    );
    run(
        r#"
        typedef unsigned short int Half;
        T identity<T>(T value) { return value; }
        int main() {
            int long unsigned first = 42;
            short signed int small = -32768;
            unsigned long long widest = 18446744073709551615;
            widest += 1;
            print(identity<unsigned long>(first)); print(identity<signed short>(small)); print(widest);
            print(sizeof(long) == sizeof(unsigned long)); print(sizeof(long long) == 8);
            Half half = 65535; print(half); return 0;
        }
    "#,
        "42\n-32768\n0\n1\n1\n65535\n",
    );
}

#[test]
fn native_integer_function_pointers_use_exact_c_types() {
    run(
        r#"
        extern long labs(long value);
        extern long long llabs(long long value);
        typedef long (*LongFunction)(long);
        typedef long long (*WideFunction)(long long);
        int main() {
            LongFunction narrow = labs; WideFunction wide = llabs;
            unsafe { print(narrow(-2147483647)); print(wide(-9223372036854775807)); }
            return 0;
        }
    "#,
        "2147483647\n9223372036854775807\n",
    );
}

#[test]
fn native_signed_overflow_and_narrowing_report_runtime_errors() {
    for body in [
        "long limit = (long)(~(unsigned long)0 / 2); print(limit + 1);",
        "long long limit = 9223372036854775807; print(limit + 1);",
        "long long value = 32768; short small = (short)value; print(small);",
    ] {
        let output = common::tracked_run(&format!("int main() {{ {body} return 0; }}"));
        assert!(!output.status.success(), "should fail {body}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("runtime error"));
    }
}

#[test]
fn rejects_conflicting_or_repeated_c_integer_specifiers() {
    for ty in [
        "unsigned unsigned",
        "signed unsigned",
        "short long",
        "long long long",
        "int char",
        "long char",
        "short short",
        "int int",
    ] {
        assert!(
            crust::check(&format!("int main() {{ {ty} value = 0; return 0; }}")).is_err(),
            "should reject {ty}"
        );
    }
}

#[test]
fn compatible_typedefs_and_extern_declarations_can_repeat() {
    run(
        r#"
        typedef short Count;
        typedef int16_t Count;
        typedef Count Count;
        typedef long (*Callback)(long);
        typedef long (*Callback)(long value);
        extern long labs(long first);
        extern long labs(long second);
        extern long labs(long);
        int main() { Count count = 7; print(count); Callback call = labs; unsafe { print(call(-42)); } return 0; }
    "#,
        "7\n42\n",
    );
    for source in [
        "extern int f(int value); extern int64_t f(int value);",
        "extern int f(int value); extern int f(char value);",
        "extern int f(int value, ...); extern int f(int value);",
        "extern int f(int value); int f(int value) { return value; }",
        "typedef int Count; typedef char Count;",
        "typedef int (*Callback)(int); typedef int (*Callback)(int, ...);",
    ] {
        assert!(
            crust::check(&format!("{source} int main() {{ return 0; }}")).is_err(),
            "should reject {source}"
        );
    }
}

#[test]
fn suffixed_literals_preserve_native_types_wraparound_and_checked_conversions() {
    run(
        r#"
        T identity<T>(T value) { return value; }
        int main() {
            print(4294967295u + 1u); print(-1u); print(-1ULL);
            print(18446744073709551615ULL + 1ull);
            print(-9223372036854775807LL - 1LL);
            print(sizeof(42L) == sizeof(long)); print(sizeof(42LL) == sizeof(long long));
            print(sizeof(+42u) == sizeof(unsigned int));
            print(identity<unsigned long>(42LU));
            uint8_t byte = 255u; print(byte);
            unsigned long long wide = 42u; print(wide);
            unsafe {
                int* pointer = 0u; print(pointer == (int*)0ULL);
                pointer = true ? pointer : 0UL;
                pointer = false ? 0ll : pointer;
                print(pointer == (int*)-0U);
            }
            return 0;
        }
        "#,
        "0\n4294967295\n18446744073709551615\n0\n-9223372036854775808\n1\n1\n1\n42\n255\n42\n1\n1\n",
    );
    let output =
        common::tracked_run("int main() { print(9223372036854775807LL + 1LL); return 0; }");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("integer overflow"));
}

#[test]
fn literal_suffix_types_follow_native_rank_and_decimal_or_nondecimal_candidates() {
    let long64 = std::mem::size_of::<std::ffi::c_long>() == 8;
    for (literal, expected) in [
        ("42u", "unsigned int"),
        ("42L", "long"),
        ("42UL", "unsigned long"),
        ("42ll", "long long"),
        ("42LLU", "unsigned long long"),
        (
            "4294967296u",
            if long64 {
                "unsigned long"
            } else {
                "unsigned long long"
            },
        ),
        ("4294967295L", if long64 { "long" } else { "long long" }),
        ("0xffffffffL", if long64 { "long" } else { "unsigned long" }),
        (
            "0xffffffffffffffffL",
            if long64 {
                "unsigned long"
            } else {
                "unsigned long long"
            },
        ),
        ("0xffffffffffffffffll", "unsigned long long"),
        (
            "9223372036854775807L",
            if long64 { "long" } else { "long long" },
        ),
        (
            "18446744073709551615U",
            if long64 {
                "unsigned long"
            } else {
                "unsigned long long"
            },
        ),
        ("-1u", "unsigned int"),
        ("+1L", "long"),
        ("-1LL", "long long"),
    ] {
        let source = format!(
            "int main() {{ {expected} expected = 0; print(expected); print({literal}); return 0; }}"
        );
        let analysis = crust::analyze(&source).unwrap();
        let native_type = analysis
            .type_at(0, source.rfind("expected").unwrap())
            .unwrap();
        let literal_type = analysis.type_at(0, source.rfind(literal).unwrap()).unwrap();
        assert_eq!(
            literal_type.description, native_type.description,
            "{literal}"
        );
    }
    // A destination conversion retains the explicit source type in editor information.
    let source = "int main() { long expected = 0; unsigned long long wide = 42L; print(expected); return 0; }";
    let analysis = crust::analyze(source).unwrap();
    assert_eq!(
        analysis
            .type_at(0, source.find("42L").unwrap())
            .unwrap()
            .description,
        analysis
            .type_at(0, source.rfind("expected").unwrap())
            .unwrap()
            .description
    );
}

#[test]
fn suffixed_constant_expressions_work_in_enums_arrays_and_generic_types() {
    run(
        r#"
        enum Values : unsigned long long {
            Wrapped = 4294967295u + 1u,
            All = ~0ULL,
            Negated = -1u,
            Selected = true ? 7uL : 1uL / 0uL,
            Count = (0xffu & 3U) + 1ULL
        };
        typedef int Row[(int)Values::Count];
        struct Holder<T> { T value; };
        int main() {
            Row values = { 10, 20, 30, 40 };
            Holder<int[2UL + 2UL]> holder = { .value = { 1, 2, 3, 4 } };
            print((unsigned long long)Values::Wrapped);
            print((unsigned long long)Values::All);
            print((unsigned long long)Values::Negated);
            print((int)Values::Selected);
            print(values[3u]); print(holder.value[3LL]);
            return 0;
        }
        "#,
        "0\n18446744073709551615\n4294967295\n7\n40\n4\n",
    );
}

#[test]
fn suffixed_literals_reject_out_of_range_types_and_destinations() {
    for body in [
        "print(9223372036854775808LL);",
        "print(-9223372036854775808LL);",
        "print(sizeof(18446744073709551615L));",
        "uint8_t byte = 256U;",
        "int value = 4294967295u;",
        "long long value = 18446744073709551615ULL;",
        "unsigned value = -1LL;",
        "uint8_t value = -1u;",
        "int* pointer = 1u;",
    ] {
        assert!(
            crust::check(&format!("int main() {{ unsafe {{ {body} }} return 0; }}")).is_err(),
            "{body}"
        );
    }
    for declaration in [
        "enum Values { Overflow = 2147483647L + 1L };",
        "enum Values : long long { Overflow = 9223372036854775807LL + 1LL };",
        "typedef int Row[1u << 32u];",
        "typedef int Row[4294967295u + 1u];",
    ] {
        assert!(
            crust::check(&format!("{declaration} int main() {{ return 0; }}")).is_err(),
            "{declaration}"
        );
    }
}

#[test]
fn variadic_c_calls_receive_the_exact_native_type_of_suffixed_literals() {
    let source = r#"
        extern unsigned int read_uint(int, ...);
        extern long read_long(int, ...);
        extern unsigned long read_ulong(int, ...);
        extern long long read_wide(int, ...);
        extern unsigned long long read_uwide(int, ...);
        int main() {
            unsafe {
                print(read_uint(0, -1u));
                print(read_long(0, 42L)); print(read_long(0, -42l));
                print(read_long(0, +42L));
                print(read_ulong(0, 42lu));
                print(read_wide(0, -9223372036854775807LL));
                print(read_wide(0, +42ll));
                print(read_uwide(0, 18446744073709551615uLL));
                print(read_uwide(0, 0xffffffffffffffffLL));
            }
            return 0;
        }
    "#;
    let native = r#"
        #include <stdarg.h>
        #define READER(name, type) type name(int tag, ...) { \
            va_list args; va_start(args, tag); type value = va_arg(args, type); \
            va_end(args); return value; }
        READER(read_uint, unsigned int)
        READER(read_long, long)
        READER(read_ulong, unsigned long)
        READER(read_wide, long long)
        READER(read_uwide, unsigned long long)
        #undef READER
    "#;
    // Check the emitted argument types before default promotions. Width-only
    // runtime checks cannot distinguish Windows long from int.
    let checks = r#"
        #define CHECK_ARGUMENT(type, value) ((void)sizeof(char[_Generic((value), type: 1, default: -1)]))
        #define read_uint(tag, value) (CHECK_ARGUMENT(unsigned int, value), (read_uint)(tag, value))
        #define read_long(tag, value) (CHECK_ARGUMENT(long, value), (read_long)(tag, value))
        #define read_ulong(tag, value) (CHECK_ARGUMENT(unsigned long, value), (read_ulong)(tag, value))
        #define read_wide(tag, value) (CHECK_ARGUMENT(long long, value), (read_wide)(tag, value))
        #define read_uwide(tag, value) (CHECK_ARGUMENT(unsigned long long, value), (read_uwide)(tag, value))
    "#;
    let generated = crust::compile(source).unwrap();
    let (declarations, main) = generated.split_once("\nint main(void)\n").unwrap();
    let output = common::tracked_generated_c(
        &format!("{native}\n{declarations}\n{checks}\nint main(void)\n{main}"),
        false,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "4294967295\n42\n-42\n42\n42\n-9223372036854775807\n42\n18446744073709551615\n18446744073709551615\n"
    );
    assert!(output.stderr.is_empty());
}
