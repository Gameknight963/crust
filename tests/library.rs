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
