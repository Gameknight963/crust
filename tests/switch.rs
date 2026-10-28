mod common;

#[test]
fn integers_enums_fallthrough_and_selector_side_effects_execute() {
    let output = common::tracked_run(
        r#"
        enum E { First = 1, Second = 2 };
        enum class S { First = 3, Second = 4 };
        int main() {
            int value = 1;
            switch(value++) { case 0: print(0); break; case 1: print(10); case 2: print(20); break; default: print(99); }
            print(value);
            E e = Second;
            switch(e) { case First: print(1); break; case Second: print(2); break; }
            S s = S::Second;
            switch(s) { case S::First: print(3); break; case S::Second: print(4); break; }
            uint64_t wide = 18446744073709551615u;
            switch(wide) { case -1: print(5); break; default: print(99); }
            int signed_value = -1;
            switch(signed_value) { case 4294967295u: print(6); break; }
            return 0;
        }
    "#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "10\n20\n2\n2\n4\n5\n6\n"
    );
}

#[test]
fn switch_rejects_duplicate_converted_values_and_nonconstant_labels() {
    for source in [
        "int main() { int n=0; switch(n) { case -1: break; case 4294967295u: break; } return 0; }",
        "int main() { int n=0; switch(n) { case n: break; } return 0; }",
        "enum E { A }; int main() { E A=E::A; switch(A) { case A: break; } return 0; }",
        "enum class E { A }; int main() { E e=E::A; switch(e) { case A: break; } return 0; }",
        "enum class E { A }; enum class F { A }; int main() { E e=E::A; switch(e) { case F::A: break; } return 0; }",
        "int main() { int n=0; switch(n) { case n++: break; } return 0; }",
    ] {
        assert!(crust::compile(source).is_err(), "accepted {source}");
    }
}

#[test]
fn integer_switch_cleans_up_owners_and_continues_the_enclosing_for_loop() {
    let output = common::tracked_run(
        "int main() { for (int i=0;i<3;i++) { BoxInt outer=box_int(i); switch(i) { case 1: { BoxInt inner=box_int(11); continue; } default: print(i); break; } } return 0; }",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "0\n2\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace('\r', ""),
        "drop:0\ndrop:11\ndrop:1\ndrop:2\n"
    );
}
