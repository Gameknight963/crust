mod common;
use common::compiler as crust;

#[test]
fn raw_addresses_check_creation_permissions_and_keep_unsafe_uses_explicit() {
    let output = common::tracked_run(
        r#"
        int main() {
            int value=42;
            int* first=&value; int* second=&value;
            print(first==second);
            {
                const int& shared=&const value;
                const int* address=&value;
                print(address==first); print(shared);
            }
            {
                int& exclusive=&value;
                int* address=&exclusive;
                print(address==first); exclusive=7;
            }
            print(value);
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
        "1\n1\n42\n1\n7\n"
    );
    for body in [
        "const int& shared=&const value; int* address=&value; print(shared);",
        "int& exclusive=&value; const int* address=&value; print(exclusive);",
        "const int& shared=&const value; int* address=&shared;",
    ] {
        let error =
            crust::check(&format!("int main(){{int value=42;{body}return 0;}} ")).unwrap_err();
        assert!(
            error.message.contains("borrow") || error.message.contains("const"),
            "{body}: {error}"
        );
    }
    for body in [
        "print(*pointer);",
        "print(pointer[0]);",
        "(*pointer)++;",
        "pointer++;",
        "int* next=pointer+1;",
        "void* erased=(void*)pointer;",
        "release(pointer);",
        "print(callback(1));",
        "Choice choice={.number=42}; print(choice.number);",
    ] {
        let source = format!(
            r#"
            extern void release(void* value);
            typedef int Callback(int);
            union Choice {{int number; double real;}};
            int identity(int value) {{return value;}}
            int main(){{int value=42;int* pointer=&value;Callback* callback=identity;{body}return 0;}}
        "#
        );
        let error = crust::check(&source).unwrap_err();
        assert!(
            error.message.contains("requires an unsafe block"),
            "{body}: {error}"
        );
    }
}

#[test]
fn references_cannot_escape_through_nested_owners_and_control_flow() {
    let declarations = r#"
        struct View {const int& value;};
        View wrap(const int& value) {return (View){.value=value};}
        const int& choose(const int& first,const int& second,bool select) borrows(first,second) {
            if(select) return first; return second;
        }
    "#;
    for body in [
        "Box<View> bad(const int& input) {int local=42;return Box<View>::create(wrap(&const local));}",
        "View bad(const int& input) {int local=42;return wrap(choose(input,&const local,true));}",
        "View bad(const int& input) {View outer=wrap(input);while(true){int local=42;outer=wrap(&const local);break;}return outer;}",
        "View bad(const int& input) {View outer=wrap(input);do{int local=42;outer=wrap(&const local);continue;}while(false);return outer;}",
        "View bad(const int& input) {View outer=wrap(input);switch(1){case 1:{int local=42;outer=wrap(&const local);break;}default:break;}return outer;}",
    ] {
        let error =
            crust::check(&format!("{declarations}{body} int main(){{return 0;}} ")).unwrap_err();
        assert!(
            error.message.contains("outlive") || error.message.contains("borrow sources"),
            "{body}: {error}"
        );
    }
}
