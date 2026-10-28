mod common;
use common::compiler as crust;

#[test]
fn returned_borrows_select_sources_and_support_reborrowing_and_forwarding() {
    let source = r#"
        int& choose(bool first, int& a, int& b) borrows(a, b) {
            if (first) return a;
            return b;
        }
        int& forward(bool first, int& a, int& b) borrows(a, b) {
            return choose(first, a, b);
        }
        const int& read(const int& a, const int& b) borrows(a) { return a; }
        int main() {
            int a=10; int b=20;
            { int& selected=forward(false, &a, &b); selected += 2;
              { const int& shared=&selected; print(shared); }
              selected += 1;
            }
            print(a); print(b);
            { const int& selected=read(&a, &b); b=99; print(selected); }
            print(b);
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
        "22\n10\n23\n10\n99\n"
    );
}

#[test]
fn every_possible_origin_remains_borrowed_and_must_be_declared() {
    let prefix = "int& choose(int& a, int& b) borrows(a,b) { return b; }";
    for statement in ["a=1;", "b=1;", "int& other=&a;", "int& other=&b;"] {
        let source = format!(
            "{prefix} int main() {{ int a=1; int b=2; int& selected=choose(&a,&b); {statement} print(selected); return 0; }}"
        );
        assert!(crust::compile(&source).is_err(), "accepted {statement}");
    }
    for body in [
        "int& bad(int& a, int& b) borrows(a) { return b; }",
        "int& bad(int& a, int& b) borrows(a) { return choose(a,b); }",
        "int& bad(int& a) { int local=1; return choose(a,&local); }",
        "int& bad(int& a, int& b) borrows(missing) { return a; }",
        "int& bad(int& a, int& b) borrows(a,a) { return a; }",
        "int& bad(int& a, const int& b) borrows(a,b) { return a; }",
        "int bad(int& a) borrows(a) { return 1; }",
    ] {
        assert!(
            crust::compile(&format!("{prefix} {body} int main() {{ return 0; }}")).is_err(),
            "accepted {body}"
        );
    }
}

#[test]
fn projected_multi_origin_borrows_preserve_disjoint_fields() {
    let source = r#"
        struct Pair { int x; int y; int& get() { return &this.x; } };
        Pair& choose(Pair& a, Pair& b) borrows(a,b) { return b; }
        int main() {
            Pair a={.x=1,.y=2}; Pair b={.x=3,.y=4};
            { Pair& pair=choose(&a,&b); int& value=pair.get(); value=42; print(value); }
            print(b.x); return 0;
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
        "42\n42\n"
    );
}

#[test]
fn choosing_an_owner_preserves_cleanup_and_freezes_both_sources() {
    let source = "Box<int>& choose(Box<int>& a, Box<int>& b) borrows(a,b) { return b; } int main() { Box<int> a=Box<int>::create(10); Box<int> b=Box<int>::create(20); { Box<int>& selected=choose(&a,&b); selected=Box<int>::create(42); print(selected.get()); } print(a.get()); print(b.get()); return 0; }";
    let output = common::tracked_run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n10\n42\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace('\r', ""),
        "drop:20\ndrop:42\ndrop:10\n"
    );
    let invalid = "Box<int>& choose(Box<int>& a, Box<int>& b) borrows(a,b) { return b; } int main() { Box<int> a=Box<int>::create(10); Box<int> b=Box<int>::create(20); Box<int>& selected=choose(&a,&b); Box<int> consumed=move(b); print(selected.get()); return 0; }";
    assert!(crust::compile(invalid).is_err());
}

#[test]
fn generic_and_instance_contracts_resolve_named_sources() {
    let source = r#"
        T& choose<T>(T& a, T& b) borrows(a,b) { return b; }
        struct Item {
            int value;
            const int& get(const int& other) const borrows(this) { return &this.value; }
        };
        int main() {
            int a=1; int b=2;
            { int& chosen=choose<int>(&a,&b); chosen=7; }
            Item item={.value=42};
            const int& answer=item.get(&a);
            a=100;
            print(answer); print(b); return 0;
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
        "42\n7\n"
    );
}

#[test]
fn contract_names_participate_in_navigation_rename_and_signature_help() {
    let source = "const int& first(const int& a, const int& b) borrows(a) { return a; } int main() { int x=1; int y=2; print(first(&x,&y)); return 0; }";
    let analysis = crust::analyze(source).unwrap();
    let declaration = source.find("a,").unwrap();
    let references = analysis.references_at(0, declaration, true);
    assert_eq!(references.len(), 3);
    let edits = analysis.rename(0, declaration, "left").unwrap();
    assert_eq!(edits.len(), 3);
    let call = source.rfind("first(&x").unwrap() + "first(".len();
    let help = analysis.signature_help_at(0, call).unwrap();
    assert!(help.signatures[0].label.ends_with("borrows(a)"));
}
