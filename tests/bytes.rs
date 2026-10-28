mod common;

fn sources(body: &str) -> crust::source::SourceSet {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let entry = root.join("target/bytes-example.cr");
    let text = format!("import \"../lib/bytes.cr\"; {body}");
    return crust::source::SourceSet::load_with(&entry, |path| {
        if path == entry {
            return Ok(text.clone());
        }
        return std::fs::read_to_string(path);
    })
    .unwrap();
}

#[test]
fn byte_slice_printing_uses_the_full_length_including_embedded_zeroes() {
    let source = sources(
        r#"
        int main() {
            uint8_t[3] data={65,0,66};
            ByteBuffer bytes=ByteBuffer::copy(&data);
            print(bytes.view());
            { uint8_t[]& view=bytes.view_mut(); print(view); }
            bytes.clear(); print(bytes.view()); return 0;
        }
    "#,
    );
    let output = common::tracked_generated_c(&source.compile().unwrap(), true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "A\0B\nA\0B\n\n"
    );
    assert!(
        crust::compile(
            "void show(const int[]& values) { print(values); } int main() { return 0; }"
        )
        .is_err()
    );
}

#[test]
fn byte_buffers_copy_append_mutate_and_clear_binary_data() {
    let source = sources(
        r#"
        int main() {
            uint8_t[3] original={0,128,255};
            ByteBuffer bytes=ByteBuffer::copy(&original);
            original[1]=1;
            print(bytes.count()); print(bytes.view()[1]);
            { uint8_t[]& view=bytes.view_mut(); view[0]=42; }
            bytes.append(&original);
            print(bytes.count()); print(bytes.view()[0]); print(bytes.view()[4]);
            bytes.clear(); print(bytes.count()); bytes.append(&original);
            print(bytes.view()[2]); return 0;
        }
    "#,
    );
    let output = common::tracked_generated_c(&source.compile().unwrap(), true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "3\n128\n6\n42\n1\n0\n255\n"
    );
}

#[test]
fn failed_byte_copy_and_append_preserve_the_source_and_destination() {
    let source = sources(
        r#"
        extern void crust_test_fail_allocations_after(size_t successes);
        int main() {
            uint8_t[2] source={10,20};
            ByteBuffer bytes=ByteBuffer::copy(&source);
            unsafe { crust_test_fail_allocations_after(0); }
            Result<Unit,AllocError> appended=bytes.try_append(&source);
            print((int)appended.error_value()); print(bytes.count());
            print(bytes.view()[0]); print(bytes.view()[1]);
            Result<ByteBuffer,AllocError> copied=ByteBuffer::try_copy(&source);
            print((int)copied.error_value()); print(source[0]);
            ByteBuffer empty=ByteBuffer::create();
            Result<ByteBuffer,AllocError> zero=ByteBuffer::try_copy(empty.view());
            print(zero.is_ok()); return 0;
        }
    "#,
    );
    let output = common::tracked_generated_c(&source.compile().unwrap(), true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "0\n2\n10\n20\n0\n10\n1\n"
    );
}

#[test]
fn byte_views_prevent_mutation_and_owner_moves_until_their_scope_ends() {
    for operation in [
        "bytes.clear();",
        "bytes.append(&source);",
        "ByteBuffer other=move(bytes);",
    ] {
        let source = sources(&format!(
            "int main() {{ uint8_t[1] source={{42}}; ByteBuffer bytes=ByteBuffer::copy(&source); const uint8_t[]& view=bytes.view(); {operation} print(view[0]); return 0; }}"
        ));
        assert!(source.check().is_err(), "accepted {operation}");
    }
}
