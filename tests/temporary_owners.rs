mod common;
use common::compiler as crust;

const PROBE: &str = r#"
    struct Probe {
        Box<int> value;
        static Self create(int value) { return (Self){.value=Box<int>::create(value)}; }
        const int& borrow() const { return this.value.get(); }
        ~Probe() { print(this.value.get()+100); }
    };
    int sum(const int& first, const int& second) { return first+second; }
"#;

#[test]
fn temporary_owner_borrows_last_through_calls_and_drop_in_reverse_construction_order() {
    let output = common::tracked_run(&format!(
        r#"{PROBE}
        int copied() {{ return Probe::create(5).borrow(); }}
        int main() {{
            print(sum(Probe::create(1).borrow(), Probe::create(2).borrow()));
            int value=Probe::create(3).borrow(); print(value);
            if (Probe::create(4).borrow()==4) print(4);
            print(copied());
            return 0;
        }}
    "#
    ));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "3\n102\n101\n103\n3\n104\n4\n105\n5\n"
    );
}

#[test]
fn temporary_owners_clean_up_across_branches_loops_and_short_circuiting() {
    let output = common::tracked_run(&format!(
        r#"{PROBE}
        int main() {{
            print(Probe::create(0).borrow() && Probe::create(99).borrow());
            print(Probe::create(0).borrow() ? Probe::create(10).borrow() : Probe::create(20).borrow());
            int remaining=2;
            while (Probe::create(remaining--).borrow()) print(9);
            switch (Probe::create(7).borrow()) {{ case 7: print(7); break; default: break; }}
            do {{ print(8); }} while (Probe::create(0).borrow());
            return 0;
        }}
    "#
    ));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "0\n100\n20\n120\n100\n102\n9\n101\n9\n100\n107\n7\n8\n100\n"
    );
}

#[test]
fn references_to_temporary_owners_cannot_escape_the_expression() {
    for body in [
        "const int& value=Probe::create(1).borrow(); print(value);",
        "const Probe& value=&const Probe::create(1);",
        "View value={.value=Probe::create(1).borrow()}; print(value.value);",
        "View value=wrap(Probe::create(1).borrow()); print(value.value);",
    ] {
        let source = format!(
            "{PROBE} struct View {{const int& value;}}; View wrap(const int& value) {{return (View){{.value=value}};}} int main(){{{body}return 0;}}"
        );
        assert!(crust::check(&source).is_err(), "{body}");
    }
    assert!(crust::check(&format!("{PROBE} const int& bad(const int& input){{return Probe::create(1).borrow();}} int main(){{return 0;}} ")).is_err());
}

#[test]
fn nested_construction_and_unevaluated_receivers_preserve_cleanup_order() {
    let output = common::tracked_run(&format!(
        r#"{PROBE}
        int main() {{
            print(sizeof(Probe::create(99).borrow()));
            print(Probe::create(Probe::create(1).borrow()+10).borrow());
            return 0;
        }}
    "#
    ));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "4\n11\n111\n101\n"
    );
}

#[test]
fn temporary_holders_keep_captured_sources_borrowed_until_cleanup() {
    let declarations = r#"
        struct Watch {
            int& value;
            static Self create(int& value) { return (Self){.value=value}; }
            int read() const { return this.value; }
            ~Watch() { this.value++; }
        };
    "#;
    let output = common::tracked_run(&format!(
        "{declarations} int main(){{ int n=1; print(Watch::create(&n).read()); print(n); if(Watch::create(&n).read()) print(n); return 0;}}"
    ));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "1\n2\n3\n"
    );
    for body in [
        "print(Watch::create(&n).read()+n);",
        "print(Watch::create(&n).read() && n);",
        "print(Watch::create(&n).read() ? n : 0);",
    ] {
        assert!(
            crust::check(&format!(
                "{declarations} int main(){{int n=1;{body}return 0;}}"
            ))
            .is_err(),
            "{body}"
        );
    }
}

#[test]
fn copyable_temporary_structs_support_borrowed_instance_methods() {
    let output = common::tracked_run(
        "struct A { int n; static Self create(){return (Self){.n=41};} int increment(){return ++this.n;} }; int main(){print(A::create().increment());return 0;}",
    );
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n"
    );
}

#[test]
fn compound_literal_receivers_use_expression_cleanup() {
    let output = common::tracked_run(&format!(
        "{PROBE} int main(){{print(((Probe){{.value=Box<int>::create(8)}}).borrow());return 0;}}"
    ));
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "8\n108\n"
    );
}
