mod common;

use std::path::Path;

fn sources(body: &str) -> crust::source::SourceSet {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let entry = root.join("target/library-example.cr");
    let text = format!("import \"../lib/box.cr\"; import \"../lib/vector.cr\"; {body}");
    return crust::source::SourceSet::load_with(&entry, |path| {
        if path.ends_with("library-example.cr") {
            return Ok(text.clone());
        }
        return std::fs::read_to_string(path);
    })
    .unwrap();
}

#[test]
fn shared_library_owns_nested_values_and_releases_each_allocation() {
    let source = sources(
        "int main() { Vector<Box<int>> values = Vector<Box<int>>::create(); values.push(Box<int>::create(42)); print(values.at(0).get()); Box<int> last = values.pop(); print(move(last).take()); return 0; }",
    );
    let output = common::tracked_generated_c(&source.compile().unwrap(), true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n42\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace('\r', ""),
        format!(
            "drop:bytes:4\ndrop:bytes:{}\n",
            std::mem::size_of::<usize>() * 2
        )
    );
}

#[test]
fn shared_library_borrows_prevent_owner_mutation_and_moves() {
    for statement in ["values.clear();", "Vector<Box<int>> other = move(values);"] {
        let body = format!(
            "int main() {{ Vector<Box<int>> values = Vector<Box<int>>::create(); values.push(Box<int>::create(42)); const Box<int>& view = values.at(0); {statement} print(view.get()); return 0; }}"
        );
        assert!(sources(&body).check().is_err(), "accepted {statement}");
    }
}

#[test]
fn container_slices_keep_their_storage_alive_and_allow_checked_mutation() {
    let source = sources(
        "int sum(const int[]& values) { int total = 0; for (size_t i = 0; i < values.length; i++) { total += values[i]; } return total; } int main() { Vector<int> values = Vector<int>::create(); print(values.view().length); values.push(10); values.push(20); { int[]& view = values.view_mut(); view[0] = 40; print(sum(view)); } print(sum(values.view())); return 0; }",
    );
    let output = common::tracked_generated_c(&source.compile().unwrap(), true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "0\n60\n60\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace('\r', ""),
        "drop:bytes:8\n"
    );
    for statement in [
        "values.push(3);",
        "values.clear();",
        "Vector<int> other = move(values);",
    ] {
        let body = format!(
            "int main() {{ Vector<int> values = Vector<int>::create(); values.push(42); const int[]& view = values.view(); {statement} print(view[0]); return 0; }}"
        );
        assert!(sources(&body).check().is_err(), "accepted {statement}");
    }
}

#[test]
fn replacing_an_owner_through_a_slice_destroys_it_once() {
    let source = sources(
        "int main() { Vector<Box<int>> values = Vector<Box<int>>::create(); values.push(Box<int>::create(10)); { Box<int>[]& view = values.view_mut(); view[0] = Box<int>::create(20); print(view[0].get()); } print(values.at(0).get()); return 0; }",
    );
    let output = common::tracked_generated_c(&source.compile().unwrap(), true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "20\n20\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace('\r', ""),
        format!(
            "drop:bytes:4\ndrop:bytes:4\ndrop:bytes:{}\n",
            std::mem::size_of::<usize>() * 2
        )
    );
}
