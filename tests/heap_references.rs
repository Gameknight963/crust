mod common;
use common::compiler as crust;

fn declarations() -> String {
    let mut source = [
        include_str!("../lib/memory.cr"),
        include_str!("../lib/result.cr"),
        include_str!("../lib/allocation.cr"),
        include_str!("../lib/box.cr"),
        include_str!("../lib/buffer.cr"),
        include_str!("../lib/vector.cr"),
    ]
    .join("\n");
    for name in ["memory", "result", "allocation"] {
        source = source.replace(&format!("import \"{name}.cr\";"), "");
    }
    return source;
}

#[test]
fn boxes_buffers_and_vectors_store_shared_views_and_keep_their_sources() {
    let source = format!(
        r#"{}
        struct View{{const int& value;}};
        int main(){{
            int first=42;int second=7;
            Box<View> owner=Box<View>::create((View){{.value=&const first}});
            print(owner.get().value);
            View extracted=move(owner).take();print(extracted.value);
            Buffer<View> copied=Buffer<View>::create(3,extracted);print(copied.at(1).value);
            Vector<View> values=Vector<View>::create();values.push(extracted);
            values.push((View){{.value=&const second}});values.reserve(16);
            print(values.at(0).value);print(values.at(1).value);
            {{View removed=values.pop();print(removed.value);}}
            values.push(extracted);print(values.count());
            return 0;
        }}
    "#,
        declarations()
    );
    let output = common::tracked_run(&source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n42\n42\n42\n7\n7\n2\n"
    );
}

#[test]
fn heap_views_reject_dangling_sources_and_conflicting_mutation() {
    let declarations = format!(
        "{} struct View{{const int& value;}};void store(Vector<View>& target,const int& value){{target.push((View){{.value=value}});}}",
        declarations()
    );
    for body in [
        "Box<View> value=Box<View>::create((View){.value=&const first});first=3;",
        "Buffer<View> value=Buffer<View>::create(2,(View){.value=&const first});first=3;",
        "Vector<View> values=Vector<View>::create();store(&values,&const first);first=3;",
        "Vector<View> values=Vector<View>::create();{int inner=3;store(&values,&const inner);}",
        "Box<View> value=Box<View>::create((View){.value=&const first});{int inner=3;value=Box<View>::create((View){.value=&const inner});}",
    ] {
        let source = format!("{declarations} int main(){{int first=1;{body}return 0;}}");
        assert!(crust::check(&source).is_err(), "{body}");
    }
}

#[test]
fn boxed_mutable_views_transfer_out_after_the_allocation_is_freed() {
    let source = format!(
        r#"{}
        struct View{{int& value;}};
        int main(){{
            int number=1;
            {{
                Box<View> owner=Box<View>::create((View){{.value=&number}});
                owner.get_mut().value=41;
                View extracted=move(owner).take();extracted.value++;
                print(extracted.value);
            }}
            print(number);return 0;
        }}
    "#,
        declarations()
    );
    let output = common::tracked_run(&source);
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
fn mutable_views_survive_vector_growth_and_consuming_removal() {
    let source = format!(
        r#"{}
        struct View{{int& value;}};
        int main(){{int number=1;{{
            Vector<View> values=Vector<View>::create();values.push((View){{.value=&number}});
            values.reserve(8);values.at_mut(0).value=41;
            {{View removed=values.remove(0);removed.value++;print(removed.value);}}
        }}print(number);return 0;}}
    "#,
        declarations()
    );
    let output = common::tracked_run(&source);
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
fn allocation_failures_return_reference_containing_inputs() {
    for operation in ["box", "vector"] {
        let insertion = if operation == "box" {
            "Result<Box<View>,AllocationFailure<View>> result=Box<View>::try_create(move(input));"
        } else {
            "Vector<View> values=Vector<View>::create();Result<Unit,AllocationFailure<View>> result=values.try_push(move(input));"
        };
        let source = format!(
            r#"{}
            extern void crust_test_fail_allocations_after(size_t successes);
            struct View{{int& value;}};
            int main(){{int number=1;{{
                View input={{.value=&number}};
                unsafe{{crust_test_fail_allocations_after(0);}}
                {insertion}
                unsafe{{crust_test_fail_allocations_after(~(size_t)0);}}
                if(result.is_ok())return 97;
                AllocationFailure<View> failure=move(result).take_error();
                View recovered=move(failure).take_value();recovered.value=42;print(recovered.value);
            }}print(number);return 0;}}
        "#,
            declarations()
        );
        let output = common::tracked_run(&source);
        assert!(
            output.status.success(),
            "{operation}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).replace('\r', ""),
            "42\n42\n"
        );
    }
}

#[test]
fn heap_view_example_uses_real_source_imports_and_frees_all_storage() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/heap_views.cr");
    let generated = crust::source::SourceSet::load(&path)
        .unwrap()
        .compile()
        .unwrap();
    let output = common::tracked_generated_c(&generated, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n7\n42\n"
    );
}
