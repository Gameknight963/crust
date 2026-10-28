mod common;

#[test]
fn globals_and_static_locals_resolve_source_symbols_without_initializer_helpers() {
    let source = "const int limit=3; int main() { static int count=1; unsafe { count+=limit; return count; } }";
    let analysis = crust::analyze(source).unwrap();
    for (name, offset) in [
        ("limit", source.rfind("limit").unwrap()),
        ("count", source.rfind("count").unwrap()),
    ] {
        let definitions = analysis.definitions_at(0, offset);
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].name, name);
    }
    assert!(
        analysis
            .symbols
            .iter()
            .all(|symbol| !symbol.name.starts_with('?') && symbol.name != "$init")
    );
}

#[test]
fn static_local_addresses_and_file_local_arrays_keep_distinct_storage() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/storage-imports/main.cr");
    let sources = crust::source::SourceSet::load_with(&root, |path| {
        return Ok(match path.file_name().unwrap().to_str().unwrap() {
            "main.cr" => "import \"a.cr\"; import \"b.cr\"; int main() { print(a()); print(a()); print(b()); return 0; }",
            "a.cr" => "static int values[2]={1,2}; int a() { static void* address=(void*)&address; unsafe { if (address != (void*)&address) return 99; values[0]++; return values[0]; } }",
            "b.cr" => "static int values[2]={10,20}; int b() { unsafe { return values[0]; } }",
            _ => return Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
        }.to_owned());
    }).unwrap();
    let output = common::tracked_generated_c(&sources.compile().unwrap(), false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "2\n3\n10\n"
    );
}

#[test]
fn global_and_static_scalars_initialize_once_and_enforce_storage_permissions() {
    let source = r#"
        const int limit = 2 + 3;
        int total;
        static int hidden = 10;
        const double fraction = 1.25 + 0.25;
        enum class Status { Ready = 2 };
        const Status status = Status::Ready;
        typedef int (*Callback)(int);
        int double_value(int n) { return n * 2; }
        static Callback callback = &double_value;
        int next() {
            static int count = 1;
            static const int step = 2;
            unsafe { count += step; return count; }
        }
        int main() {
            print(limit); print(fraction); print((int)status);
            print(next()); print(next());
            int* address = &total;
            unsafe { *address = callback(hidden); print(total); }
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
        "5\n1.5\n2\n3\n5\n20\n"
    );
}

#[test]
fn mutable_storage_and_invalid_static_initializers_are_rejected() {
    for source in [
        "int count=0; int main() { return count; }",
        "int count=0; int main() { count=1; return 0; }",
        "int count=0; int main() { int& value=&count; return value; }",
        "int main() { static int count=0; return count; }",
        "const int count=0; int main() { unsafe { count=1; } return 0; }",
        "int value() { return 1; } int count=value(); int main() { return 0; }",
        "int count=1; int copy=count; int main() { return 0; }",
        "int count=2147483647 + 1; int main() { return 0; }",
        "uint8_t count=200 + 100; int main() { return 0; }",
        "BoxInt owner=box_int(1); int main() { return 0; }",
        "int value=1; int value=2; int main() { return 0; }",
    ] {
        assert!(crust::compile(source).is_err(), "accepted {source}");
    }
}

#[test]
fn static_arrays_aggregates_strings_and_addresses_preserve_native_initialization() {
    let source = r#"
        struct Pair { int x; int y; };
        tagged union Choice { int ok; int error; };
        const int[3] values={1,2,3};
        const char[6] greeting="hello";
        const Pair point={.x=4,.y=5};
        const Choice default_choice;
        const Choice failure={.error=7};
        const int* const pointer=&values[1 + 1];
        int[2] zeroes;
        int next() {
            static int[2] local={10,20};
            unsafe { local[0]++; return local[0]; }
        }
        int main() {
            print(values[2]); print(greeting); print(point.y);
            print(default_choice.ok); print(failure.error);
            unsafe { print(*pointer); print(zeroes[1]); }
            print(next()); print(next());
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
        "3\nhello\n5\n0\n7\n3\n0\n11\n12\n"
    );
}

#[test]
fn static_defaults_preserve_private_field_invariants_and_array_bounds() {
    for source in [
        "struct Hidden { private int value; }; const Hidden value; int main() { return 0; }",
        "struct Hidden { private int value; }; const Hidden[2] values; int main() { return 0; }",
        "const int[2] values={1,2}; const int* pointer=&values[1+1]; int main() { return 0; }",
        "int main() { static int this=0; return 0; }",
        "int main() { if (true) static int count=0; return 0; }",
        "int main() { while (false) static int count=0; return 0; }",
        "int main() { do static int count=0; while (false); return 0; }",
    ] {
        assert!(crust::compile(source).is_err(), "accepted {source}");
    }
}
