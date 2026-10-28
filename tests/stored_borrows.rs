mod common;
use common::compiler as crust;

const DEFINITIONS: &str = r#"
    struct View { const int& value; };
    void set(View& target, const int& value) { target=(View){.value=value}; }
    void forward(View& target, const int& value) { set(target,value); }
    void nothing(View& first, View& second) {}
"#;

#[test]
fn stores_infer_input_lifetimes_through_forward_calls() {
    let output = common::tracked_run(&format!(
        r#"{DEFINITIONS}
        int main() {{
            int first=1; int second=42;
            View target={{.value=&const first}};
            forward(&target,&const second);
            print(target.value);
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
        "42\n"
    );
    for body in [
        "forward(&target,&const second); second=3;",
        "set(&target,&const second); second=3;",
        "{ int inner=3; set(&target,&const inner); }",
    ] {
        let source = format!(
            "{DEFINITIONS} int main(){{int first=1; int second=2; View target={{.value=&const first}};{body}return 0;}}"
        );
        assert!(crust::check(&source).is_err(), "{body}");
    }
}

#[test]
fn storing_local_and_value_parameter_addresses_in_outputs_is_rejected() {
    for function in [
        "void bad(View& target) { int local=1; target=(View){.value=&const local}; }",
        "void bad(View& target, int value) { target=(View){.value=&const value}; }",
        "void bad(View& target, const int& value) { int local=1; set(target,&const local); }",
    ] {
        assert!(
            crust::check(&format!("{DEFINITIONS} {function} int main(){{return 0;}}")).is_err(),
            "{function}"
        );
    }
}

#[test]
fn unrelated_mutable_parameters_do_not_acquire_storage_relationships() {
    let source = format!(
        "{DEFINITIONS} int main(){{int first=1;int second=2;{{View a={{.value=&const first}};View b={{.value=&const second}};nothing(&a,&b);}}first=3;second=4;return 0;}}"
    );
    crust::check(&source).unwrap();
}

#[test]
fn storage_effects_follow_arrays_nested_values_and_referenced_destinations() {
    let definitions = format!(
        r#"{DEFINITIONS}
        struct Outer {{ View view; }};
        struct Indirect {{ View& view; }};
        void nested(Outer& target,const int& value) {{set(&target.view,value);}}
        void indirect(Indirect& target,const int& value) {{set(target.view,value);}}
        void consumed(Indirect target,const int& value) {{set(target.view,value);}}
    "#
    );
    for body in [
        "View values[2]={{.value=&const first},{.value=&const first}};set(&values[0],&const second);second=3;",
        "Outer target={.view={.value=&const first}};nested(&target,&const second);second=3;",
        "View target={.value=&const first};{Indirect holder={.view=&target};indirect(&holder,&const second);}second=3;",
        "View target={.value=&const first};{Indirect holder={.view=&target};consumed(move(holder),&const second);}second=3;",
        "View target={.value=&const first};{int inner=2;Indirect holder={.view=&target};indirect(&holder,&const inner);}",
    ] {
        let source =
            format!("{definitions} int main(){{int first=1;int second=2;{body}return 0;}}");
        assert!(crust::check(&source).is_err(), "{body}");
    }
    let output = common::tracked_run(&format!(
        "{definitions} int main(){{int first=1;int second=42;View target={{.value=&const first}};{{Indirect holder={{.view=&target}};indirect(&holder,&const second);}}print(target.value);return 0;}}"
    ));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n"
    );
}

#[test]
fn retained_shared_references_allow_reads_and_preserve_self_source_effects() {
    let source = r#"
        struct View {const int& value;};
        struct Owner {int number; View view;};
        void shared(View& target,int& source){target=(View){.value=&const source};}
        void own(Owner& target){target.view=(View){.value=&const target.number};}
        int main(){int first=1;int second=42;View target={.value=&const first};shared(&target,&second);print(second);print(target.value);return 0;}
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
    let source = "struct View{const int& value;};struct Owner{int number;View view;};void own(Owner& target){target.view=(View){.value=&const target.number};}int main(){int first=1;Owner target={.number=42,.view={.value=&const first}};own(&target);target.number=3;return 0;}";
    assert!(crust::check(source).is_err());
}

#[test]
fn mutable_retention_transfers_with_the_holder_and_recursive_calls_reach_a_fixed_point() {
    let source = r#"
        struct View {int& value;};
        void set(View& target,int& value){target=(View){.value=value};}
        void recurse(View& target,int& value,int count){if(count>0) recurse(target,value,count-1);else set(target,value);}
        int main(){int first=1;int second=2;{View target={.value=&first};recurse(&target,&second,3);View moved=move(target);moved.value=42;print(moved.value);}print(second);return 0;}
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
    assert!(
        crust::check(&source.replace(
            "View moved=move(target);",
            "second=3;View moved=move(target);"
        ))
        .is_err()
    );
}

#[test]
fn mutually_exclusive_stores_merge_without_conflicting_with_each_other() {
    let definitions = r#"
        struct View {int& value;};
        void set(View& target,int& value){target=(View){.value=value};}
        void branch(View& target,int& value,bool first){if(first)set(target,value);else set(target,value);}
        void select(View& target,int& value,int choice){switch(choice){case 1:set(target,value);break;default:set(target,value);break;}}
        void conditional(View& target,int& value,bool first){first ? set(target,value) : set(target,value);}
    "#;
    for function in [
        "branch(&target,&second,true)",
        "select(&target,&second,1)",
        "conditional(&target,&second,false)",
    ] {
        let source = format!(
            "{definitions} int main(){{int first=1;int second=2;{{View target={{.value=&first}};{function};target.value=42;print(target.value);}}print(second);return 0;}}"
        );
        let output = common::tracked_run(&source);
        assert!(
            output.status.success(),
            "{function}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).replace('\r', ""),
            "42\n42\n"
        );
    }
}

#[test]
fn destructor_stores_cannot_escape_implicit_cleanup() {
    let source = format!(
        r#"{DEFINITIONS}
        struct Writer {{
            View& target; const int& value;
            ~Writer() {{set(this.target,this.value);}}
        }};
        int main(){{int first=1;View target={{.value=&const first}};{{int inner=42;Writer writer={{.target=&target,.value=&const inner}};}}print(target.value);return 0;}}
    "#
    );
    let error = crust::check(&source).unwrap_err();
    assert!(
        error.message.contains("destructors cannot retain"),
        "{error}"
    );
}

#[test]
fn copying_stored_references_within_one_input_keeps_every_possible_source() {
    let declarations = r#"
        struct View{const int& value;};
        struct Pair{View first;View second;};
        void copy(Pair& target){target.first=target.second;}
    "#;
    let source = format!(
        "{declarations} int main(){{int first=1;int second=42;Pair pair={{.first={{.value=&const first}},.second={{.value=&const second}}}};copy(&pair);print(pair.first.value);return 0;}}"
    );
    let output = common::tracked_run(&source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n"
    );
    let source = format!(
        "{declarations} int main(){{int first=1;View out={{.value=&const first}};{{int second=42;Pair pair={{.first={{.value=&const first}},.second={{.value=&const second}}}};copy(&pair);out=pair.first;}}print(out.value);return 0;}}"
    );
    assert!(crust::check(&source).is_err());
}
