mod common;

#[test]
fn floating_literals_arithmetic_references_conditions_and_c_promotions_execute() {
    let source = r#"
        extern int printf(const char* format, ...);
        void add(float& value) { value += 0.5f; }
        double twice(double value) { return value * 2; }
        int main() {
            float first = .5f;
            add(&first);
            print(first++);
            print(++first);
            double second = 1.e1 + 0x1.8p1;
            second /= 2;
            print(second);
            print(twice(1.25f));
            long double wide = 2.5L;
            wide += 1;
            print(wide);
            print((int)-2.75);
            print((uint64_t)-0.5);
            print((int)-2147483648.5);
            print((int64_t)-0x1p63);
            print((uint64_t)0x1.fffffffffffffp63);
            print((bool)0.25);
            print(1 ? first : 2.0);
            if (0.5 && !0.0) { print(-first < -1.0); }
            char[8] format = "%g %Lg\n";
            unsafe { printf(&format[0], 1.25f, wide); }
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
        "1\n3\n6.5\n2.5\n3.5\n-2\n0\n-2147483648\n-9223372036854775808\n18446744073709549568\n1\n3\n1\n1.25 3.5\n"
    );
}

#[test]
fn floating_conversions_reject_nan_and_values_outside_integer_or_float_ranges() {
    for expression in [
        "(int)(0.0 / 0.0)",
        "(int)(1.0 / 0.0)",
        "(int)2147483648.0",
        "(uint64_t)0x1p64",
        "(int64_t)0x1p63",
        "(uint32_t)-1.0",
        "(float)1e100",
    ] {
        let source = format!("int main() {{ {expression}; return 0; }}");
        let output = common::tracked_run(&source);
        assert!(!output.status.success(), "accepted {expression}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("conversion out of range"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn floating_values_reject_integer_only_operations_and_invalid_literals() {
    for source in [
        "int main() { double value=1.0; value %= 2; return 0; }",
        "int main() { print(1.0 & 1); return 0; }",
        "int main() { print(1.0 << 1); return 0; }",
        "int main() { print(~1.0); return 0; }",
        "int main() { double[1.5] values={0.0}; return 0; }",
        "int main() { print(1e+); return 0; }",
        "int main() { print(0x1.f); return 0; }",
        "int main() { print(1.0ff); return 0; }",
    ] {
        assert!(crust::compile(source).is_err(), "accepted {source}");
    }
}
