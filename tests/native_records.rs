mod common;
use common::compiler as crust;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
fn header(source: &str) -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/native-record-tests")
        .join(std::process::id().to_string())
        .join(NEXT.fetch_add(1, Ordering::Relaxed).to_string());
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("records.h");
    std::fs::write(&path, source).unwrap();
    return path;
}
fn native_header(path: &std::path::Path) -> String {
    return format!(
        "extern header {};",
        serde_json::to_string(&path.to_string_lossy().replace('\\', "/")).unwrap()
    );
}

#[test]
fn complete_native_records_preserve_layout_by_value_calls_and_packed_field_borrows() {
    let path = header(
        r#"
        #include <stdint.h>
        struct Point { int x; int y; };
        #pragma pack(push,1)
        struct Packed { char tag; struct Point point; int value; };
        #pragma pack(pop)
        struct Aligned { _Alignas(64) int value; };
        union Number { uint64_t bits; double number; };
        static inline struct Point translate(struct Point point,int delta) {point.x+=delta;point.y+=delta;return point;}
        static inline int native_sum(struct Point point) {return point.x+point.y;}
    "#,
    );
    let source = format!(
        r#"{}
        extern struct Point{{int x;int y;}};
        extern struct Packed{{char tag;Point point;int value;}};
        extern struct Aligned{{int value;}};
        extern union Number{{uint64_t bits;double number;}};
        extern Point translate(Point point,int delta);
        extern int native_sum(Point point);
        void increment(int& value){{value++;}}
        int sum(const Point& point){{return point.x+point.y;}}
        int main(){{
            Packed items[2]={{{{.tag=1,.point={{.x=10,.y=20}},.value=30}},{{.tag=2,.point={{.x=40,.y=1}},.value=9}}}};
            increment(&items[0].value);increment(&items[1].point.x);
            print(sum(&const items[1].point));print(items[0].value);
            unsafe{{Point moved=translate(items[0].point,1);print(native_sum(moved));}}
            print(sizeof(Packed));print(alignof(Packed));print(alignof(Aligned));
            Number number={{.bits=42}};unsafe{{print(number.bits);}}
            return 0;
        }}
    "#,
        native_header(&path)
    );
    let output = common::tracked_run(&source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n31\n32\n13\n1\n64\n42\n"
    );
}

#[test]
fn generic_heap_owners_honor_native_record_alignment() {
    let path = header(
        "struct Aligned { _Alignas(64) int value; }; static inline int aligned_address(const struct Aligned *value) { return ((unsigned long long)value & 63) == 0; }",
    );
    let memory = include_str!("../lib/memory.cr");
    let allocation = common::allocation_source();
    let boxed = include_str!("../lib/box.cr")
        .replace("import \"memory.cr\";", memory)
        .replace("import \"allocation.cr\";", &allocation);
    let vector = include_str!("../lib/vector.cr")
        .replace("import \"memory.cr\";", "")
        .replace("import \"allocation.cr\";", "");
    let source = format!(
        r#"{} {boxed} {vector}
        extern struct Aligned{{int value;}};
        extern int aligned_address(const Aligned* value);
        int main(){{Box<Aligned> boxed=Box<Aligned>::create((Aligned){{.value=42}});
            Vector<Aligned> values=Vector<Aligned>::create();values.push((Aligned){{.value=7}});values.reserve(16);
            unsafe{{print(aligned_address(&boxed.get()));print(aligned_address(&values.at(0)));}}
            print(boxed.get().value);print(values.at(0).value);return 0;
        }}
    "#,
        native_header(&path)
    );
    let output = common::tracked_run(&source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "1\n1\n42\n7\n"
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr).lines().count(), 3);
}

#[test]
fn native_field_access_and_static_initializers_preserve_header_macros() {
    let path = header(
        r#"
        #include <stdio.h>
        #pragma push_macro("stderr")
        #undef stderr
        struct MacroRecord { int stderr; int score; };
        #pragma pop_macro("stderr")
        #define score 123
        #define value 789
        #define alignment 999
        #define a 111
        #define b 222
        #define MacroRecord 456
    "#,
    );
    let source = format!(
        r#"{}
        extern struct MacroRecord{{int stderr;int score;}};
        const MacroRecord stored={{.stderr=40,.score=1}};
        int main(){{MacroRecord value={{.stderr=42,.score=7}};print(value.stderr);print(value.score);print(stored.stderr);return 0;}}
    "#,
        native_header(&path)
    );
    let output = common::tracked_run(&source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n7\n40\n"
    );
}

#[test]
fn native_compilation_rejects_incorrect_field_types_and_overlapping_struct_projections() {
    for (native, declaration, expected) in [
        (
            "struct Record{int value;};",
            "extern struct Record{long value;};",
            "native field type mismatch",
        ),
        (
            "struct Record{union{int first;int second;};};",
            "extern struct Record{int first;int second;};",
            "nonoverlapping",
        ),
        (
            "struct Record{int first;int second;};",
            "extern struct Record{int second;int first;};",
            "header order",
        ),
    ] {
        let path = header(native);
        let generated = crust::compile(&format!(
            "{} {declaration} int main(){{return 0;}}",
            native_header(&path)
        ))
        .unwrap();
        let source = path.with_extension("c");
        let output = path.with_extension(std::env::consts::EXE_EXTENSION);
        std::fs::write(&source, generated).unwrap();
        let error = crust::native::compile_file_with_options(&source, &output, &Default::default())
            .unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn imports_complete_records_by_value_and_renames_keyword_tags_and_fields() {
    let path = header(
        "struct unsafe {int init;int c_init;};static inline struct unsafe create_record(int value){return (struct unsafe){value,7};}",
    );
    let bindings = crust::bindings::generate(
        &path,
        &crust::bindings::Options {
            structs: vec!["unsafe".to_owned()],
            functions: vec!["create_record".to_owned()],
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        bindings.contains("extern struct c_unsafe = \"unsafe\""),
        "{bindings}"
    );
    assert!(bindings.contains("c_init_ = \"init\""), "{bindings}");
    let output = common::tracked_run(&format!(
        "{bindings} int main(){{unsafe{{c_unsafe value=create_record(42);print(value.c_init_);print(value.c_init);}}return 0;}}"
    ));
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
fn native_record_and_field_rename_preserves_c_names() {
    let path = header("struct Record{int value;};");
    let mut source = format!(
        "{} extern struct Record{{int value;}};int main(){{Record data={{.value=42}};print(data.value);return 0;}}",
        native_header(&path)
    );
    for (old, new) in [("Record{", "Point"), ("value;", "score")] {
        let analysis = crust::analyze(&source).unwrap();
        let edits = analysis.rename(0, source.find(old).unwrap(), new).unwrap();
        for edit in edits.iter().rev() {
            source.replace_range(
                edit.location.span.start..edit.location.span.end,
                &edit.new_text,
            );
        }
    }
    assert!(source.contains("Point = \"Record\""), "{source}");
    assert!(source.contains("score = \"value\""), "{source}");
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
}

#[test]
fn cli_record_imports_preserve_callbacks_and_variadic_aggregate_arguments() {
    let path = header(
        r#"
        #include <stdarg.h>
        struct Point{int x;int y;};
        union Number{unsigned long long bits;double number;};
        static inline struct Point apply(struct Point (*callback)(struct Point),struct Point value){return callback(value);}
        static inline int variadic_sum(int count,...){va_list args;va_start(args,count);int result=0;for(int i=0;i<count;i++){struct Point value=va_arg(args,struct Point);result+=value.x+value.y;}va_end(args);return result;}
    "#,
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_crust"))
        .arg("import-c")
        .arg(&path)
        .args([
            "--struct",
            "Point",
            "--union",
            "Number",
            "--function",
            "apply",
            "--function",
            "variadic_sum",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bindings = String::from_utf8(output.stdout).unwrap();
    assert!(bindings.contains("extern struct Point"));
    assert!(bindings.contains("extern union Number"));
    let source = format!(
        "{bindings} Point shift(Point value){{value.x++;return value;}} int main(){{Point value={{.x=41,.y=7}};unsafe{{Point changed=apply(&shift,value);print(changed.x);print(variadic_sum(2,value,changed));}}return 0;}}"
    );
    let output = common::tracked_run(&source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n97\n"
    );
}

#[test]
fn selected_records_validate_tags_fields_and_complete_dependencies() {
    let path = header(
        "struct Inner{int value;};struct Outer{struct Inner inner;};union Choice{int value;double number;};struct Qualified{const int value;};struct Bits{unsigned int value:3;};",
    );
    for options in [
        crust::bindings::Options {
            structs: vec!["Missing".to_owned()],
            ..Default::default()
        },
        crust::bindings::Options {
            structs: vec!["Choice".to_owned()],
            ..Default::default()
        },
        crust::bindings::Options {
            structs: vec!["Inner".to_owned(), "Inner".to_owned()],
            ..Default::default()
        },
        crust::bindings::Options {
            structs: vec!["bad-name".to_owned()],
            ..Default::default()
        },
        crust::bindings::Options {
            structs: vec!["Outer".to_owned()],
            ..Default::default()
        },
        crust::bindings::Options {
            structs: vec!["Bits".to_owned()],
            ..Default::default()
        },
    ] {
        assert!(
            crust::bindings::generate(&path, &options).is_err(),
            "{options:?}"
        );
    }
    let bindings = crust::bindings::generate(
        &path,
        &crust::bindings::Options {
            structs: vec!["Outer".to_owned(), "Inner".to_owned()],
            ..Default::default()
        },
    )
    .unwrap();
    let output = common::tracked_run(&format!(
        "{bindings} int main(){{Outer value={{.inner={{.value=42}}}};print(value.inner.value);return 0;}}"
    ));
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n"
    );
}

#[test]
fn exported_native_records_use_the_original_header_in_a_separate_c_caller() {
    let path = header("struct Point{int x;int y;};");
    let source = format!(
        "{} extern struct Point{{int x;int y;}};extern Point shifted(Point value){{value.x++;return value;}}",
        native_header(&path)
    );
    let public = crust::emit_header(&source, crust::ProgramKind::Library).unwrap();
    assert!(public.contains("records.h"), "{public}");
    let directory = path.parent().unwrap();
    let header = directory.join("api.h");
    std::fs::write(&header, public).unwrap();
    let generated = directory.join("library.c");
    std::fs::write(
        &generated,
        crust::compile_with_kind(&source, crust::ProgramKind::Library).unwrap(),
    )
    .unwrap();
    let caller = directory.join("caller.c");
    std::fs::write(&caller,"#include \"api.h\"\nint main(void){struct Point value=shifted((struct Point){41,7});return value.x==42&&value.y==7?0:1;}").unwrap();
    let executable = directory.join(format!("caller{}", std::env::consts::EXE_SUFFIX));
    crust::native::compile_file_with_options(
        &generated,
        &executable,
        &crust::native::Options {
            sources: vec![caller],
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        std::process::Command::new(executable)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn native_packed_records_support_source_methods_and_nested_destructors() {
    let path = header(
        "struct Child{int value;};\n#pragma pack(push,1)\nstruct Parent{unsigned char tag;struct Child child;};\n#pragma pack(pop)\n#define child 789\n",
    );
    let source = format!(
        r#"{}
        extern struct Child{{int score = "value";int read()const{{return this.score;}}~Child(){{print(this.score);}}}};
        extern struct Parent{{unsigned char tag;Child nested = "child";}};
        int main(){{Parent values[2]={{{{.tag=1,.nested={{.score=41}}}},{{.tag=2,.nested={{.score=42}}}}}};print(values[0].nested.read());return 0;}}
    "#,
        native_header(&path)
    );
    let output = common::tracked_run(&source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "41\n42\n41\n"
    );
}

#[test]
fn records_project_example_imports_its_native_layout() {
    common::assert_example(
        "examples/records_project/main.cr",
        "42\n5\n1\n43\n",
        "",
        false,
    );
}

#[test]
fn native_arrays_preserve_layout_copies_borrows_and_static_initializers() {
    let path = header(
        "struct Item { int n; };\n#pragma pack(push,1)\nstruct Packet { char tag; int values[3]; int matrix[2][2]; char text[4]; struct Item items[2]; };\n#pragma pack(pop)\nstatic inline int read_packet(struct Packet p) { return p.values[1]+p.values[2]; }\n",
    );
    let bindings = crust::bindings::generate(
        &path,
        &crust::bindings::Options {
            structs: vec!["Packet".to_owned(), "Item".to_owned()],
            functions: vec!["read_packet".to_owned()],
            ..Default::default()
        },
    )
    .unwrap();
    assert!(bindings.contains("matrix[2][2]"), "{bindings}");
    let source = format!(
        r#"{bindings}
        const Packet template = {{.tag=1,.values={{1,2,3}},.matrix={{{{4,5}},{{6,7}}}},.text="abc",.items={{{{.n=8}},{{.n=9}}}}}};
        const int* address = &template.matrix[1][0];
        Packet echo(Packet value) {{ return value; }}
        int main() {{
            Packet packet=echo(template);
            packet.values[1]=40;packet.values[2]=2;
            {{ int[3]& values=&packet.values;values[0]=10; }}
            print(packet.values[1]+packet.values[2]);print(packet.matrix[1][0]);
            print(packet.items[1].n);print(packet.text[2]);
            unsafe {{ const int* actual=&template.matrix[1][0];if(address != actual) return 1; }}
            print(template.matrix[1][0]);unsafe {{ print(read_packet(packet)); }}
            int copy[3]=packet.values;copy[1]=1;
            print(packet.values[1]);packet.values=copy;print(packet.values[1]);
            return 0;
        }}
    "#
    );
    let output = common::tracked_run(&source);
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n6\n9\n99\n6\n42\n40\n1\n"
    );
}

#[test]
fn native_qualified_fields_preserve_const_permissions_and_volatile_borrows() {
    let path = header(
        "#pragma pack(push,1)\nstruct Sample { const int id; volatile int ticks; const int frozen[2]; volatile int samples[2]; int *const pointer; };\n#pragma pack(pop)\n",
    );
    let bindings = crust::bindings::generate(
        &path,
        &crust::bindings::Options {
            structs: vec!["Sample".to_owned()],
            ..Default::default()
        },
    )
    .unwrap();
    let source = format!(
        r#"{bindings}
        void increment(volatile int& value) {{ value++; }}
        int read(const Sample& value) {{ return value.ticks + value.samples[0]; }}
        int read_volatile(const volatile Sample& value) {{ return value.ticks + value.samples[0]; }}
        int main() {{
            int target=8;
            Sample value={{.id=42,.ticks=1,.frozen={{3,4}},.samples={{5,6}},.pointer=&target}};
            increment(&value.ticks);increment(&value.samples[0]);
            print(value.id);print(value.frozen[1]);print(read(&const value));
            unsafe {{ volatile Sample* raw=&value;print(read_volatile(&const *raw));*value.pointer=9; }} print(target);
            Sample copied=value;print(copied.id);print(copied.samples[0]);return 0;
        }}
    "#
    );
    let output = common::tracked_run(&source);
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n4\n8\n8\n9\n42\n6\n"
    );
    for operation in [
        "value.id=1;",
        "value.id++;",
        "value.id+=1;",
        "value.frozen[0]=1;",
        "value.pointer=0;",
        "int& writable=&value.id;",
        "int[2]& writable=&value.frozen;",
        "value=value;",
        "Sample& alias=&value;alias=value;",
        "int& writable=&value.ticks;",
        "int[2]& writable=&value.samples;",
    ] {
        let source = format!(
            "{bindings} int main() {{ int target=0;Sample value={{.id=42,.ticks=0,.frozen={{1,2}},.samples={{3,4}},.pointer=&target}};{operation} return 0; }}"
        );
        assert!(crust::check(&source).is_err(), "{operation}");
    }
}

#[test]
fn native_field_restrictions_report_frontend_errors() {
    for source in [
        "struct Owner { ~Owner() {} };extern struct Native {const Owner owner;};int main(){return 0;}",
        "struct Owner { ~Owner() {} };extern struct Native {volatile Owner owner;};int main(){return 0;}",
        "extern struct Native {const int values[4097];};int main(){Native value={.values={1}};return 0;}",
    ] {
        assert!(crust::check(source).is_err(), "{source}");
    }
    for declaration in [
        "struct Native {int count;int values[];};",
        "struct Native {int values[0];};",
    ] {
        let path = header(declaration);
        assert!(
            crust::bindings::generate(
                &path,
                &crust::bindings::Options {
                    structs: vec!["Native".to_owned()],
                    ..Default::default()
                },
            )
            .is_err(),
            "{declaration}"
        );
    }
}

#[test]
fn const_native_records_support_factory_returns_temporaries_and_heap_initialization() {
    let path = header("struct Frozen { const int id; };\n");
    let memory = include_str!("../lib/memory.cr");
    let allocation = common::allocation_source();
    let boxed = include_str!("../lib/box.cr")
        .replace("import \"memory.cr\";", memory)
        .replace("import \"allocation.cr\";", &allocation);
    let buffer = include_str!("../lib/buffer.cr")
        .replace("import \"memory.cr\";", "")
        .replace("import \"allocation.cr\";", "");
    let source = format!(
        r#"{} {boxed} {buffer}
        extern struct Frozen {{const int id;int get() const {{return this.id;}}}};
        Frozen create(int value) {{return (Frozen){{.id=value}};}}
        int read(const Frozen& value) {{return value.id;}}
        int main() {{
            Frozen selected=true?create(42):create(7);print(selected.id);
            print(read(&(true?create(3):create(4))));print(create(5).get());
            bool test=true&&create(6).get()==6;print(test);
            for(int i=0;i<2;i++) {{print(create(i).get());}}
            Box<Frozen> box=Box<Frozen>::create(create(8));print(box.get().id);
            Buffer<Frozen> values=Buffer<Frozen>::create(2,create(9));print(values.at(1).id);
            return 0;
        }}
    "#,
        native_header(&path)
    );
    let output = common::tracked_run(&source);
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n3\n5\n1\n0\n1\n8\n9\n"
    );
}

#[test]
fn native_array_fields_preserve_typedefs_callbacks_and_const_pointer_objects() {
    let path = header(
        "typedef int Row[2];struct Table { Row init[2]; int (*callbacks[1])(int); int *const targets[1]; const int *shared[1]; };\n",
    );
    let bindings = crust::bindings::generate(
        &path,
        &crust::bindings::Options {
            structs: vec!["Table".to_owned()],
            ..Default::default()
        },
    )
    .unwrap();
    let source = format!(
        r#"{bindings}
        int increment(int value) {{return value+1;}}
        int main() {{
            int target=1;
            Table table={{.c_init={{{{2,3}},{{4,5}}}},.callbacks={{increment}},.targets={{&target}},.shared={{&target}}}};
            print(table.c_init[1][1]);
            unsafe {{print(table.callbacks[0](41));*table.targets[0]=9;print(*table.shared[0]);}}
            return 0;
        }}
    "#
    );
    let output = common::tracked_run(&source);
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "5\n42\n9\n"
    );
}
