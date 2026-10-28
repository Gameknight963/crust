mod common;
use common::compiler as crust;

fn run(source: &str, expected: &str) {
    let output = common::tracked_run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        expected
    );
}

#[test]
fn shared_reference_fields_copy_nest_and_return_with_inferred_sources() {
    run(
        r#"
struct View { const int& value; int own; int read() const { return this.value; } };
struct Nested { View item; };
View view(const int& value) { return (View){ .value = value, .own = 9 }; }
View forward(View input) { return input; }
const int& extract(View input) { return input.value; }
int main() {
    int value = 42;
    { View first = view(&value); View second = forward(first);
      Nested nested = { .item = second };
      print(nested.item.value); print(nested.item.read()); print(extract(first)); }
    value = 43; print(value);
    return 0;
}
"#,
        "42\n42\n42\n43\n",
    );
}

#[test]
fn mutable_reference_fields_are_aliases_and_move_with_their_holder() {
    run(
        r#"
struct View { int& value; void increment() { this.value += 1; } };
View view(int& value) { return (View){ .value = value }; }
View forward(View input) { return move(input); }
int main() {
    int value = 40;
    { View first = view(&value); View second = forward(move(first));
      second.increment(); second.value++; print(second.value);
      { const int& read = second.value; print(read); }
    }
    print(value); return 0;
}
"#,
        "42\n42\n42\n",
    );
}

#[test]
fn slice_fields_retain_bounds_and_shared_or_mutable_access() {
    run(
        r#"
struct Read { const int[]& values; };
struct Write { int[]& values; };
int main() {
    int[3] values = { 1, 2, 3 };
    { Write writable = { .values = &values }; writable.values[1] = 42;
      print(writable.values.length); print(writable.values[1]); }
    { Read readable = { .values = &values }; print(readable.values[1]); }
    return 0;
}
"#,
        "3\n42\n42\n",
    );
}

#[test]
fn reference_field_lifetimes_reject_dangling_views_aliasing_and_invalid_returns() {
    let prefix = "struct View { int& value; int own; }; struct Read { const int& value; };";
    for body in [
        "View bad() { int local=1; return (View){.value=&local,.own=0}; }",
        "Read bad(Read input) { int local=1; return (Read){.value=&local}; }",
        "const int& bad(Read input) { return &input; }",
        "const int& bad(View input) { return &input.own; }",
        "View bad(int& a,int& b) borrows(a) { return (View){.value=b,.own=0}; }",
        "void main_body() { int value=1; View first={.value=&value,.own=0}; View second=first; }",
        "void main_body() { int value=1; View first={.value=&value,.own=0}; value=2; }",
        "void main_body() { int value=1; View first={.value=&value,.own=0}; int& part=first.value; View second=move(first); }",
        "void main_body() { int a=1; int b=2; Read view={.value=&a}; {int local=3; view=(Read){.value=&local};} }",
        "void main_body() { int value=1; View view={.value=&value,.own=0}; const View& shared=&view; shared.value=2; }",
        "void main_body() { int value=1; Read view={.value=&value}; view.value=2; }",
    ] {
        let source = format!("{prefix} {body} int main() {{ return 0; }}");
        assert!(crust::compile(&source).is_err(), "accepted {body}");
    }
}

#[test]
fn selecting_borrowed_holders_retains_every_possible_field_source() {
    let prefix = r#"
struct Read { const int& value; };
Read& choose(Read& a, Read& b) borrows(a,b) { return b; }
"#;
    run(
        &format!(
            "{prefix} int main() {{ int x=1; int y=2; {{ Read a={{.value=&x}}; Read b={{.value=&y}}; const int& selected=choose(&a,&b).value; print(selected); }} y=3; print(y); return 0; }}"
        ),
        "2\n3\n",
    );
    for expression in [
        "const int& selected=choose(&a,&b).value;",
        "Read selected=choose(&a,&b);",
    ] {
        let source = format!(
            "{prefix} int main() {{ int x=1; int y=2; Read a={{.value=&x}}; Read b={{.value=&y}}; {expression} y=3; return 0; }}"
        );
        assert!(crust::compile(&source).is_err(), "accepted {expression}");
    }
}

#[test]
fn generic_views_tagged_payloads_and_conditionals_keep_their_reference_sources() {
    let prefix = format!(
        "{}\nstruct View<T> {{ const T& value; static Self create(const T& input) {{ return (Self){{.value=input}}; }} }};",
        include_str!("../lib/result.cr")
    );
    run(
        &format!(
            r#"{prefix}
Result<View<int>, int> make(const int& value) {{ return Result<View<int>, int>::success(View<int>::create(value)); }}
int main() {{
    int a=40; int b=42;
    {{ View<int> first={{.value=&a}}; View<int> second={{.value=&b}};
       View<int> selected=false ? first : second; print(selected.value);
       Result<View<int>,int> result=make(&b); View<int> extracted=result.take_value(); print(extracted.value);
       Result<View<int>,int> error=Result<View<int>,int>::failure(3); print(error.take_error()); }}
    b=43; print(b); return 0;
}}
"#
        ),
        "42\n42\n3\n43\n",
    );
    for statement in [
        "Result<View<int>,int> result=Result<View<int>,int>::success(View<int>::create(&b)); b=9;",
        "View<int> first={.value=&a}; View<int> second={.value=&b}; View<int> selected=false?first:second; b=9;",
    ] {
        let source = format!("{prefix} int main() {{ int a=1; int b=2; {statement} return 0; }}");
        assert!(crust::compile(&source).is_err(), "accepted {statement}");
    }
}

#[test]
fn slices_of_views_and_multi_source_returns_preserve_nested_lifetimes() {
    let prefix = r#"
struct View { const int& value; };
View choose(bool first, const int& a, const int& b) borrows(a,b) {
    return first ? (View){.value=a} : (View){.value=b};
}
View from_slice(const View[]& values) { return values[0]; }
"#;
    run(
        &format!(
            r#"{prefix}
int main() {{
    int a=1; int b=42;
    {{ View[2] views={{{{.value=&a}},{{.value=&b}}}};
       View first=from_slice(&views); print(first.value);
       View selected=choose(false,&a,&b); print(selected.value); }}
    a=3; b=4; print(a+b); return 0;
}}
"#
        ),
        "1\n42\n7\n",
    );
    let invalid = format!(
        r#"{prefix}
View bad(const View[]& values, const int& other) borrows(other) {{ return values[0]; }}
int main() {{ return 0; }}
"#
    );
    assert!(crust::compile(&invalid).is_err());
}

#[test]
fn reference_fields_coexist_with_owned_fields_and_independent_local_projections() {
    let output = common::tracked_run(
        r#"
struct Mixed { Box<int> owner; const int& value; };
struct Pair { int& first; int& second; };
int main() {
    int x=1; int y=2;
    { Mixed first={.owner=Box<int>::create(7),.value=&x};
      const int& shared=&x; Mixed second=move(first); print(second.value); print(shared); }
    { Pair pair={.first=&x,.second=&y};
      int& first=pair.first; pair.second=42; first=3; print(first); print(pair.second); }
    print(x+y); return 0;
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
        "1\n1\n3\n42\n45\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace('\r', ""),
        "drop:7\n"
    );
}

#[test]
fn replacement_retains_branch_origins_and_unsupported_storage_is_rejected() {
    let prefix = "struct View { const int& value; };";
    run(
        &format!(
            "{prefix} int main() {{ int a=1; int b=42; {{ View view={{.value=&a}}; if (true) view=(View){{.value=&b}}; print(view.value); }} b=43; print(b); return 0; }}"
        ),
        "42\n43\n",
    );
    for mutation in ["a=9;", "b=9;"] {
        let source = format!(
            "{prefix} int main() {{ int a=1; int b=2; View view={{.value=&a}}; if (true) view=(View){{.value=&b}}; {mutation} return 0; }}"
        );
        assert!(crust::compile(&source).is_err(), "accepted {mutation}");
    }
    for source in [
        "struct View { const int& value; }; View* pointer; int main() { return 0; }",
        "union View { const int& value; int number; }; int main() { return 0; }",
    ] {
        assert!(crust::compile(source).is_err(), "accepted {source}");
    }
}

#[test]
fn borrowing_a_holder_checks_reborrows_of_its_stored_referents() {
    let prefix = "struct View { int& value; }; int read(const View& view) { return view.value; } void write(View& view) { view.value=3; }";
    for statement in [
        "int& child=view.value; const View& alias=&view; print(alias.value);",
        "int& child=view.value; print(read(&view));",
        "const int& child=view.value; write(&view);",
    ] {
        let source = format!(
            "{prefix} int main() {{ int n=1; View view={{.value=&n}}; {statement} return 0; }}"
        );
        assert!(crust::compile(&source).is_err(), "accepted {statement}");
    }
    run(
        &format!(
            "{prefix} int main() {{ int n=1; {{View view={{.value=&n}}; {{const int& child=view.value; print(child);}} View& alias=&view; write(alias); print(read(alias)); }} print(n); return 0; }}"
        ),
        "1\n3\n3\n",
    );
}
