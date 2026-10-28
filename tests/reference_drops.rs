mod common;

fn run(source: &str, stdout: &str, stderr: &str) {
    let output = common::tracked_run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        stdout
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace('\r', ""),
        stderr
    );
}

#[test]
fn reference_destructors_run_once_after_moves_and_before_source_cleanup() {
    run(
        r#"
struct Watch { const BoxInt& source; ~Watch() { print(*this.source); } };
Watch create(const BoxInt& source) { return (Watch){.source=source}; }
Watch forward(Watch input) { return move(input); }
int main() {
    BoxInt first=box_int(1); BoxInt second=box_int(2);
    Watch watch=forward(create(&first));
    watch=(Watch){.source=&second};
    return 0;
}
"#,
        "1\n2\n",
        "drop:2\ndrop:1\n",
    );
}

#[test]
fn readonly_destructors_allow_escaping_shared_references_to_external_sources() {
    run(
        r#"
struct Watch { const int& value; ~Watch() { print(this.value); } };
struct View { const int& value; };
const int& extract(Watch watch) { return watch.value; }
int main() {
    int n=42; int initial=0;
    const int& extracted=extract((Watch){.value=&n}); print(extracted);
    View outer={.value=&initial};
    { Watch watch={.value=&n}; outer=(View){.value=watch.value}; }
    print(outer.value); return 0;
}
"#,
        "42\n42\n42\n42\n",
        "",
    );
}

#[test]
fn mutating_reference_destructors_cannot_invalidate_escaping_borrows() {
    let prefix = "struct View { const int& value; }; struct Reset { BoxInt& owner; ~Reset() { this.owner=box_int(99); } };";
    for function in [
        "const int& bad(Reset reset) { return &*reset.owner; }",
        "View bad(Reset reset) { View result={.value=&*reset.owner}; return result; }",
    ] {
        let source = format!("{prefix} {function} int main() {{ return 0; }}");
        assert!(crust::compile(&source).is_err(), "accepted {function}");
    }
    let source = r#"
struct View { const int& value; };
struct Increment { int& value; ~Increment() { this.value++; } };
int main() {
    int initial=0; int n=41;
    View outer={.value=&initial};
    { Increment counter={.value=&n}; outer=(View){.value=counter.value}; }
    print(outer.value); return 0;
}
"#;
    assert!(crust::compile(source).is_err());
}

#[test]
fn later_owners_cannot_be_stored_in_earlier_holders() {
    let source = r#"
struct Watch { const BoxInt& source; ~Watch() { print(*this.source); } };
int main() {
    BoxInt initial=box_int(1);
    Watch watch={.source=&initial};
    BoxInt later=box_int(2);
    watch=(Watch){.source=&later};
    return 0;
}
"#;
    let error = crust::compile(source).unwrap_err();
    assert!(
        error.message.contains("destroyed before its holder"),
        "{error}"
    );
}

#[test]
fn scope_exits_break_continue_and_returns_allow_expired_local_reborrows() {
    run(
        r#"
struct Increment { int& value; ~Increment() { this.value++; } };
int test(int& value) {
    Increment counter={.value=value};
    { const int& read=counter.value; print(read); }
    return 7;
}
int main() {
    int n=40;
    print(test(&n));
    for(int i=0;i<3;i++) { Increment counter={.value=&n}; if(i==0) continue; break; }
    print(n); return 0;
}
"#,
        "40\n7\n43\n",
        "",
    );
}

#[test]
fn nested_and_tagged_owners_keep_reference_destructors_and_consuming_moves() {
    let source = format!(
        r#"{}
struct Increment {{ int& value; ~Increment() {{ this.value++; }} }};
struct Nested {{ Increment inner; }};
int main() {{
    int n=40;
    {{ Nested nested={{.inner={{.value=&n}}}}; }}
    {{ Result<Increment,int> result=Result<Increment,int>::success((Increment){{.value=&n}});
       Increment value=move(result).take_value(); }}
    {{ Result<Increment,int> failure=Result<Increment,int>::failure(7); print(move(failure).take_error()); }}
    print(n); return 0;
}}
"#,
        include_str!("../lib/result.cr")
    );
    run(&source, "7\n42\n", "");
}

#[test]
fn repeated_aggregate_shapes_bound_reference_tracking_work() {
    let mut source = String::from("struct Level0 { const int& value; };\n");
    for level in 1..=13 {
        source.push_str(&format!(
            "struct Level{level} {{ Level{} left; Level{} right; }};\n",
            level - 1,
            level - 1
        ));
    }
    let parameter = format!("{source} void inspect(Level13 input) {{}} int main() {{ return 0; }}");
    let error = crust::compile(&parameter).unwrap_err();
    assert!(error.message.contains("tracking exceeds"), "{error}");
    source.push_str("int main() { int value=1; Level0 first={.value=&value};");
    for level in 1..=13 {
        let previous = if level == 1 {
            "first".to_owned()
        } else {
            format!("value{}", level - 1)
        };
        source.push_str(&format!(
            "Level{level} value{level}={{.left={previous},.right={previous}}};"
        ));
    }
    source.push_str("return 0; }");
    let error = crust::compile(&source).unwrap_err();
    assert!(error.message.contains("tracking exceeds"), "{error}");
}

#[test]
fn return_value_reads_finish_before_dropping_the_source_owner() {
    run(
        r#"
struct Resource {
    BoxInt value;
    const int& read() const { return &*this.value; }
    int finish(Self this) { return this.read(); }
};
int consume(Resource value) { return value.read(); }
int main() {
    print((Resource){.value=box_int(7)}.finish());
    print(consume((Resource){.value=box_int(8)}));
    return 0;
}
"#,
        "7\n8\n",
        "drop:7\ndrop:8\n",
    );
}
