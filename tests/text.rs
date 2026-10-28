mod common;

#[test]
fn text_example_runs_with_shared_library_imports() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/text.cr");
    let source = crust::source::SourceSet::load(&path).unwrap();
    let output = common::tracked_generated_c(&source.compile().unwrap(), true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "Hello, Crust!\nCrust\n"
    );
}

#[test]
fn compiled_utf8_validation_matches_rust_for_short_sequences_and_multibyte_boundaries() {
    let mut cases = vec![Vec::new()];
    for first in 0..=255u8 {
        cases.push(vec![first]);
        for second in 0..=255u8 {
            cases.push(vec![first, second]);
        }
    }
    for point in [
        0x7f, 0x80, 0x7ff, 0x800, 0xd7ff, 0xe000, 0xffff, 0x10000, 0x10ffff,
    ] {
        let mut buffer = [0u8; 4];
        let encoded = char::from_u32(point)
            .unwrap()
            .encode_utf8(&mut buffer)
            .as_bytes()
            .to_vec();
        cases.push(encoded.clone());
        for index in 0..encoded.len() {
            cases.push(encoded[..index].to_vec());
            for byte in 0..=255u8 {
                let mut changed = encoded.clone();
                changed[index] = byte;
                cases.push(changed);
            }
        }
    }
    let mut seed = 42u64;
    for length in 3..=8 {
        for _ in 0..256 {
            let mut bytes = Vec::new();
            for _ in 0..length {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                bytes.push((seed >> 32) as u8);
            }
            cases.push(bytes);
        }
    }
    let source = sources(
        r#"
        extern bool validate(const uint8_t* bytes, size_t length) = "crust_test_validate" {
            unsafe {
                const uint8_t[]& view = slice(bytes, length);
                return String::is_utf8(view);
            }
        }
        extern int crust_utf8_cases();
        int main() { unsafe { return crust_utf8_cases(); } }
    "#,
    );
    let mut generated = source.compile().unwrap();
    generated.push_str(
        "\nstatic const struct { uint8_t bytes[8]; size_t length; int valid; } utf8_cases[] = {\n",
    );
    for bytes in cases {
        let values = if bytes.is_empty() {
            "0".to_owned()
        } else {
            bytes
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(",")
        };
        generated.push_str(&format!(
            "{{{{{values}}},{},{}}},\n",
            bytes.len(),
            u8::from(std::str::from_utf8(&bytes).is_ok())
        ));
    }
    generated.push_str(r#"};
        int crust_utf8_cases(void) {
            for (size_t i = 0; i < sizeof(utf8_cases) / sizeof(utf8_cases[0]); i++) {
                if (crust_test_validate(utf8_cases[i].bytes, utf8_cases[i].length) != utf8_cases[i].valid) {
                    fprintf(stderr, "UTF-8 disagreement at case %zu\n", i);
                    return 1;
                }
            }
            return 0;
        }
    "#);
    let output = common::tracked_generated_c(&generated, true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
}

fn sources(body: &str) -> crust::source::SourceSet {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let entry = root.join("target/text-example.cr");
    let text = format!("import \"../lib/string.cr\"; {body}");
    return crust::source::SourceSet::load_with(&entry, |path| {
        if path == entry {
            return Ok(text.clone());
        }
        return std::fs::read_to_string(path);
    })
    .unwrap();
}

#[test]
fn strings_preserve_utf8_and_check_byte_boundaries() {
    let source = sources(
        r#"
        int main() {
            uint8_t[6] source={65,0xc3,0xa9,0xe2,0x82,0xac};
            Result<String,TextError> copied=String::try_copy(&source);
            String text=move(copied).take_value();
            print(text.view());
            print(text.count()); print(text.byte_at(1));
            print(text.is_boundary(1)); print(text.is_boundary(2));
            Result<String,TextError> selected=text.try_substring(1,2);
            String part=move(selected).take_value();
            print(part.count()); print(part.byte_at(1));
            Result<Unit,TextError> invalid=text.try_truncate(2);
            print((int)invalid.error_value()); print(text.count());
            Result<Unit,TextError> shortened=text.try_truncate(3);
            print(shortened.is_ok()); print(text.count());
            Result<Unit,TextError> appended=text.try_append(&source);
            print(appended.is_ok()); print(text.count());
            text.clear(); print(text.count());
            return 0;
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
        "A\u{e9}\u{20ac}\n6\n195\n1\n0\n2\n169\n1\n6\n1\n3\n1\n9\n0\n"
    );
}

#[test]
fn strings_recover_invalid_bytes_and_preserve_text_on_allocation_failure() {
    let source = sources(
        r#"
        extern void crust_test_fail_allocations_after(size_t successes);
        int main() {
            uint8_t[2] invalid={0xc0,0x80};
            ByteBuffer bytes=ByteBuffer::copy(&invalid);
            Result<String,ByteBuffer> decoded=String::try_from_bytes(move(bytes));
            print(decoded.is_ok());
            ByteBuffer recovered=move(decoded).take_error();
            print(recovered.count()); print(recovered.view()[0]);
            uint8_t[1] valid={65};
            Result<String,TextError> copied=String::try_copy(&valid);
            String text=move(copied).take_value();
            unsafe { crust_test_fail_allocations_after(0); }
            Result<Unit,TextError> bad=text.try_append(&invalid);
            print((int)bad.error_value());
            Result<Unit,TextError> failed=text.try_append(&valid);
            print((int)failed.error_value()); print(text.count()); print(text.byte_at(0));
            Result<String,TextError> substring=text.try_substring(0,1);
            print((int)substring.error_value());
            Result<String,TextError> outside=text.try_substring(1,~(size_t)0);
            print((int)outside.error_value());
            return 0;
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
        "0\n2\n192\n0\n3\n1\n65\n3\n1\n"
    );
}

#[test]
fn bounded_c_strings_validate_utf8_and_require_a_terminator() {
    let source = sources(
        r#"
        int main() {
            char[3] source={ (char)0x41, 0, (char)0x42 };
            Result<String,TextError> copied=String::try_from_c_string(&source);
            String text=move(copied).take_value();
            print(text.count()); print(text.byte_at(0));
            char[1] missing={'A'};
            Result<String,TextError> invalid=String::try_from_c_string(&missing);
            print((int)invalid.error_value());
            char[1] empty={0};
            Result<String,TextError> zero=String::try_from_c_string(&empty);
            String blank=move(zero).take_value(); print(blank.count());
            char[3] unicode="\xc3\xa9";
            Result<String,TextError> decoded=String::try_from_c_string(&unicode);
            String accent=move(decoded).take_value(); print(accent.count()); print(accent.byte_at(1));
            return 0;
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
        "1\n65\n2\n0\n2\n169\n"
    );
}

#[test]
fn string_views_keep_the_owner_alive_and_do_not_allow_byte_mutation() {
    for operation in ["view[0]=0xff;", "text.clear();", "String other=move(text);"] {
        let source = sources(&format!(
            "int main() {{ uint8_t[1] source={{65}}; Result<String,TextError> copied=String::try_copy(&source); String text=move(copied).take_value(); const uint8_t[]& view=text.view(); {operation} print(view[0]); return 0; }}"
        ));
        assert!(source.check().is_err(), "accepted {operation}");
    }
}
