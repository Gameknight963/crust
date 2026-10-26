mod common;

#[test]
fn enum_initializers_evaluate_flags_arithmetic_casts_and_prior_enumerators() {
    run(
        r#"
        enum Flags : uint32_t {
            Read = 1 << 0,
            Write = 1 << 1,
            ReadWrite = Read | Flags::Write,
            High = (uint32_t)1 << 31,
            All = ~(uint32_t)0,
            Same = (uint32_t)ReadWrite,
        };
        enum Values { Prior = (int)Flags::ReadWrite + 4, Product = Prior * 3, Division = Product / 2, Remainder = Product % 2, Negative = -3 >> 1, Next };
        int main() {
            print((uint32_t)ReadWrite); print((uint32_t)High); print((uint32_t)All);
            print((int)Prior); print((int)Product); print((int)Division);
            print((int)Remainder); print((int)Negative); print((int)Next);
            return 0;
        }
    "#,
        "3\n2147483648\n4294967295\n7\n21\n10\n1\n-2\n-1\n",
    );
}

#[test]
fn enum_constant_arithmetic_matches_runtime_promotions_and_unsigned_wrapping() {
    run(
        r#"
        enum Wide : uint64_t {
            Maximum = (uint64_t)18446744073709551615,
            Wrapped = Maximum + (uint64_t)1,
            Product = Maximum * Maximum,
            Negated = -(uint64_t)1,
        };
        enum Small { Promoted = (uint8_t)128 << 1, Comparison = 1 << 2 < 5, Logical = !0 + (2 != 3), Narrow = (uint8_t)7 };
        int main() {
            print((uint64_t)Wrapped); print((uint64_t)Product); print((uint64_t)Negated);
            print((int)Promoted); print((int)Comparison); print((int)Logical); print((int)Narrow);
            return 0;
        }
    "#,
        "0\n1\n18446744073709551615\n256\n1\n2\n7\n",
    );
}

#[test]
fn enum_constant_logical_operators_skip_unevaluated_arithmetic_failures() {
    run(
        r#"
        enum E { A = 0 && (1 / 0), B = 1 || (1 << -1), C = 0 && (uint8_t)256, D = 1 || (2147483647 + 1) };
        int main() { print((int)A); print((int)B); print((int)C); print((int)D); return 0; }
    "#,
        "0\n1\n0\n1\n",
    );
}

#[test]
fn invalid_enum_constant_expressions_report_frontend_errors() {
    for (initializer, expected) in [
        ("1 << 31", "overflow"),
        ("1 << -1", "shift count"),
        ("1 >> 32", "shift count"),
        ("1 / 0", "zero"),
        ("1 % 0", "zero"),
        ("(int64_t)-9223372036854775808 % -1", "overflow"),
        ("(uint8_t)256", "conversion"),
        ("-1 + (uint32_t)1", "conversion"),
        ("later", "previously declared"),
        ("value()", "constant expression"),
        ("0 && value()", "constant expression"),
        ("-18446744073709551615", "64-bit"),
    ] {
        let source = format!(
            "int value() {{ return 1; }} enum E {{ A = {initializer}, later = 1 }}; int main() {{ return 0; }}"
        );
        let error = crust::check(&source).unwrap_err();
        assert!(error.message.contains(expected), "{initializer}: {error}");
    }
}

#[test]
fn enum_constant_uses_participate_in_source_navigation_and_reference_search() {
    let source = "enum Flags { Read = 1 << 0, Write = 1 << 1, Both = Read | Flags::Write }; int main() { return (int)Both; }";
    let analysis = crust::analyze(source).unwrap();
    let offset = source.find("Read |").unwrap();
    let definitions = analysis.definitions_at(0, offset);
    assert_eq!(definitions.len(), 1);
    assert_eq!(
        definitions[0].location.span.start,
        source.find("Read =").unwrap()
    );
    assert_eq!(analysis.references_at(0, offset, true).len(), 2);
}

#[test]
fn enum_constants_resolve_imported_definitions_and_report_cycles() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/virtual-enum-constants");
    let entry = "import \"flags.cr\"; enum Combined { Both = Read | Flags::Write }; int main() { return (int)Both - 3; }";
    let helper = "enum Flags { Read = 1 << 0, Write = 1 << 1 };";
    let sources = crust::source::SourceSet::load_with(&root.join("main.cr"), |path| {
        return Ok(if path.ends_with("main.cr") {
            entry.to_owned()
        } else {
            helper.to_owned()
        });
    })
    .unwrap();
    sources.check().unwrap();
    let analysis = sources.analyze().unwrap();
    let target = analysis.definitions_at(0, entry.find("Read |").unwrap());
    assert_eq!(target[0].location.file, 1);
    let cycle =
        "enum A { First = B::Second }; enum B { Second = A::First }; int main() { return 0; }";
    assert!(crust::check(cycle).is_err());
    crust::check("enum A { Last = B::First }; enum B { First = 1, Other = A::Last }; int main() { return 0; }").unwrap();
    crust::check("enum A { Cast = (B)7 }; enum B : uint8_t { Zero }; int main() { return 0; }")
        .unwrap();
    assert!(
        crust::check(
            "enum A { Cast = (B)256 }; enum B : uint8_t { Zero }; int main() { return 0; }"
        )
        .is_err()
    );
    let mut deep = String::new();
    for i in 0..70 {
        deep.push_str(&format!("enum E{i} {{ V{i} = V{} }}; ", i + 1));
    }
    deep.push_str("enum Last { V70 = 1 }; int main() { return 0; }");
    assert!(
        crust::check(&deep)
            .unwrap_err()
            .message
            .contains("dependency depth")
    );
}

fn run(source: &str, stdout: &str) {
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
    assert!(output.stderr.is_empty());
}

#[test]
fn runs_enum_example_with_methods_scoped_and_unscoped_names() {
    run(include_str!("../examples/enums.cr"), "1\n4\n");
}

#[test]
fn enums_support_forward_uses_typedefs_arrays_and_value_returns() {
    run(
        r#"
        typedef enum Color Shade;
        Color relay(Shade value) { return value; }
        struct Palette { Color colors[2]; };
        enum Color { Red = -2, Green, Blue = 7, SameBlue = 7, };
        int main() {
            Palette p = { .colors = { Red, Shade::Blue } };
            Color copy = relay(p.colors[0]);
            print((int)copy); print((int)Green);
            print(p.colors[1] == SameBlue);
            int Red = 42; print(Red); print((int)Color::Red);
            print(sizeof(Color)); print(alignof(Shade));
            return 0;
        }
    "#,
        "-2\n-1\n1\n42\n-2\n4\n4\n",
    );
}

#[test]
fn enum_borrows_and_local_references_preserve_type_and_permissions() {
    run(
        r#"
        enum State : uint8_t { Off, On };
        void turn_on(State^ state) { *state = On; }
        int read(const State^ state) { return (int)*state; }
        int main() {
            State state = Off;
            {
                State^ reference = ^state;
                turn_on(reference);
                print(read(^const *reference));
            }
            print((int)state); print(sizeof(State));
            return 0;
        }
    "#,
        "1\n1\n1\n",
    );
}

#[test]
fn explicit_casts_check_representations_and_allow_unnamed_values() {
    run(
        r#"
        enum Byte : uint8_t { Zero, Maximum = 255 };
        enum Wide : uint64_t { Huge = 18446744073709551615 };
        enum Signed : int64_t { Minimum = -9223372036854775808 };
        int main() {
            Byte byte = (Byte)42;
            print((int)byte); print((bool)byte); print((bool)Zero);
            print((uint64_t)Huge); print((int64_t)Minimum);
            byte = (Byte)(int)Maximum; print((int)byte);
            print(Minimum < (Signed)0);
            return 0;
        }
    "#,
        "42\n1\n0\n18446744073709551615\n-9223372036854775808\n255\n1\n",
    );
    for source in [
        "enum Byte : uint8_t { Zero }; int main() { int n = -1; Byte b = (Byte)n; return 0; }",
        "enum Byte : uint8_t { Zero }; int main() { int n = 256; Byte b = (Byte)n; return 0; }",
        "enum Big : uint64_t { Maximum = 18446744073709551615 }; int main() { print((int64_t)Maximum); return 0; }",
    ] {
        let output = common::tracked_run(source);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("integer conversion out of range")
        );
    }
}

#[test]
fn rejects_invalid_definitions_values_and_name_collisions() {
    for source in [
        "enum Empty {}; int main() { return 0; }",
        "enum E { A }; enum E { B }; int main() { return 0; }",
        "enum E { A, A }; int main() { return 0; }",
        "enum E { A }; enum F { A }; int main() { return 0; }",
        "enum E { print }; int main() { return 0; }",
        "enum E { this }; int main() { return 0; }",
        "enum E { A }; void A() {} int main() { return 0; }",
        "enum E { A }; struct E { int n; }; int main() { return 0; }",
        "enum E : bool { A }; int main() { return 0; }",
        "enum E : uint8_t { A = 256 }; int main() { return 0; }",
        "enum E : uint8_t { A = -1 }; int main() { return 0; }",
        "enum E : uint8_t { A = 255, B }; int main() { return 0; }",
        "enum E { A = 2147483648 }; int main() { return 0; }",
        "enum E { A }; int main() { E a = E::Missing; return 0; }",
        "int main() { enum Missing value = 0; return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}

#[test]
fn enum_arithmetic_and_integer_conversions_require_explicit_casts() {
    let prelude = "enum E { A }; enum F { B }; void set(E^ value) {}";
    for body in [
        "E value = 0;",
        "int value = A;",
        "F value = A;",
        "E value = A; value += 1;",
        "print(A + A);",
        "print(1 + A);",
        "print(A == 0);",
        "print(A == B);",
        "print(A);",
        "if (A) {}",
        "E value = (E)box_int(1);",
        "int n = 0; set(^n);",
    ] {
        let source = format!("{prelude} int main() {{ {body} return 0; }}");
        assert!(crust::check(&source).is_err(), "should reject {source}");
    }
}
