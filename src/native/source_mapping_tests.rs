use super::*;
use serde_json::Value;

fn directory(name: &str) -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/source-mapping-tests")
        .join(std::process::id().to_string())
        .join(name);
    std::fs::create_dir_all(&directory).unwrap();
    return directory;
}

fn native_path(path: &Path) -> String {
    let name = crate::source::display_path(path);
    return if cfg!(windows) {
        name.replace('/', "\\")
    } else {
        name
    };
}

fn clang_ast(path: &Path, function: &str) -> Value {
    let mut command = Command::new("clang");
    configure_windows_toolchain(&mut command);
    let output = command
        .args([
            "-std=c11",
            "-fsyntax-only",
            "-Xclang",
            "-ast-dump=json",
            "-Xclang",
            "-ast-dump-filter",
            "-Xclang",
        ])
        .arg(function)
        .arg(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    return serde_json::Deserializer::from_slice(&output.stdout)
        .into_iter::<Value>()
        .map(Result::unwrap)
        .find(|declaration| {
            declaration["inner"].as_array().is_some_and(|children| {
                children.iter().any(|child| child["kind"] == "CompoundStmt")
            })
        })
        .unwrap();
}

fn find_kind<'a>(node: &'a Value, kind: &str) -> Option<&'a Value> {
    if node["kind"] == kind {
        return Some(node);
    }
    return node["inner"]
        .as_array()?
        .iter()
        .find_map(|child| find_kind(child, kind));
}

#[test]
fn native_locations_and_debug_records_follow_imported_source_and_closing_braces() {
    let directory = directory("imports");
    let module = directory.join("math ü.cr");
    std::fs::write(&module, "// mixed newlines\r\nint add(int value) {\r    int answer = value + 1;\r\n    return answer;\r}\r\nvoid release() {\r\n    Box<int> owner = Box<int>::create(7);\r\n}\r\n").unwrap();
    let mut text = std::fs::read_to_string(&module).unwrap();
    text.push_str("\nimport \"../../../../tests/common/box.cr\";\n");
    std::fs::write(&module, text).unwrap();
    let entry = directory.join("main.cr");
    std::fs::write(
        &entry,
        "import \"math ü.cr\";\n\nint main() {\n    release();\n    return add(41);\n}\n",
    )
    .unwrap();
    let sources = crate::source::SourceSet::load(&entry).unwrap();
    let c_path = directory.join("generated.c");
    std::fs::write(&c_path, sources.compile().unwrap()).unwrap();
    let add = clang_ast(&c_path, "crust_fn_add");
    assert_eq!(add["loc"]["presumedFile"], native_path(&module));
    assert_eq!(add["loc"]["presumedLine"], 2);
    let body = find_kind(&add, "CompoundStmt").unwrap();
    assert_eq!(body["range"]["end"]["presumedLine"], 5);
    let main = clang_ast(&c_path, "main");
    assert_eq!(main["loc"]["presumedFile"], native_path(&entry));
    assert_eq!(main["loc"]["presumedLine"], 3);

    let ir_path = directory.join("debug.ll");
    let mut command = Command::new("clang");
    configure_windows_toolchain(&mut command);
    let output = command
        .args(["-std=c11", "-g", "-S", "-emit-llvm"])
        .arg(&c_path)
        .arg("-o")
        .arg(&ir_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ir = std::fs::read_to_string(ir_path).unwrap();
    assert!(
        ir.lines()
            .any(|line| line.contains("!DILocalVariable(name: \"answer\"")
                && line.contains("line: 3,")),
        "{ir}"
    );
    assert!(
        ir.lines()
            .any(|line| line.contains("!DISubprogram(name: \"crust_fn_add\"")
                && line.contains("line: 2,")),
        "{ir}"
    );
    assert!(
        ir.lines()
            .any(|line| line.contains("!DISubprogram(name: \"main\"") && line.contains("line: 3,")),
        "{ir}"
    );
    assert!(ir.contains("!DILocation(line: 4,"));
    let release: Vec<_> = ir
        .lines()
        .skip_while(|line| !(line.starts_with("define ") && line.contains("@crust_fn_release(")))
        .take_while(|line| *line != "}")
        .collect();
    let drop = release
        .iter()
        .find(|line| line.contains("call void @crust_drop_struct_"))
        .unwrap();
    let location = drop
        .split("!dbg !")
        .nth(1)
        .unwrap()
        .split(|character: char| !character.is_ascii_digit())
        .next()
        .unwrap();
    let record = ir
        .lines()
        .find(|line| line.starts_with(&format!("!{location} = !DILocation(")))
        .unwrap();
    assert!(
        record.contains("line: 8,"),
        "cleanup should map to the closing brace: {record}"
    );
}

#[test]
fn native_line_filenames_preserve_quotes_backslashes_question_marks_and_utf8() {
    let directory = directory("filenames");
    let entry = directory.join("virtual ü ' \" ?? filename.cr");
    let sources =
        crate::source::SourceSet::load_with(&entry, |_| Ok("int main() { return 0; }".to_owned()))
            .unwrap();
    let c_path = directory.join("escaped.c");
    std::fs::write(&c_path, sources.compile().unwrap()).unwrap();
    let main = clang_ast(&c_path, "main");
    assert_eq!(main["loc"]["presumedFile"], native_path(&entry));
    assert_eq!(main["loc"]["presumedLine"], 1);
    assert!(
        !crate::compile("int main() { return 0; }")
            .unwrap()
            .contains("#line")
    );
}

#[test]
fn debug_records_keep_source_parameters_locals_fields_and_type_names() {
    let directory = directory("source-names");
    let entry = directory.join("names.cr");
    std::fs::write(
        &entry,
        r#"
struct Point { int x; int y; };
struct Pair<T> { T first; T second; };
enum class Color { Red, Blue };
int inspect(int input) {
    Point position = { .x = input, .y = 2 };
    Pair<int> pair = { .first = position.x, .second = position.y };
    Color color = Color::Red;
    int score = pair.first + pair.second;
    return score + (int)color;
}
int main() { return inspect(40); }
"#,
    )
    .unwrap();
    let sources = crate::source::SourceSet::load(&entry).unwrap();
    let c_path = directory.join("names.c");
    std::fs::write(&c_path, sources.compile().unwrap()).unwrap();
    let ir_path = directory.join("names.ll");
    let mut command = Command::new("clang");
    configure_windows_toolchain(&mut command);
    let output = command
        .args(["-std=c11", "-g", "-S", "-emit-llvm"])
        .arg(&c_path)
        .arg("-o")
        .arg(&ir_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ir = std::fs::read_to_string(ir_path).unwrap();
    for name in ["input", "position", "pair", "color", "score"] {
        assert!(
            ir.contains(&format!("!DILocalVariable(name: \"{name}\"")),
            "missing variable {name}: {ir}"
        );
    }
    for name in ["x", "y", "first", "second"] {
        assert!(
            ir.contains(&format!("DW_TAG_member, name: \"{name}\"")),
            "missing field {name}: {ir}"
        );
    }
    assert!(
        ir.contains("DW_TAG_structure_type, name: \"Point\""),
        "{ir}"
    );
    assert!(
        ir.contains("DW_TAG_typedef, name: \"crust_Color_0\""),
        "{ir}"
    );
    assert!(
        ir.contains("DW_TAG_structure_type, name: \"crust_Pair_int_"),
        "{ir}"
    );
}
