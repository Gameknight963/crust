use std::process::Command;

fn cases() -> Vec<String> {
    return vec![
        format!("int main() {{ return {}1; }}", "1 + ".repeat(5000)),
        format!(
            "int main() {{ return {}0{}; }}",
            "(".repeat(2000),
            ")".repeat(2000)
        ),
        format!("int main() {{ return {}0; }}", "! ".repeat(2000)),
        format!(
            "int main() {{{}return 0;{}}}",
            "{".repeat(2000),
            "}".repeat(2000)
        ),
        format!(
            "int main() {{{}{{ return 0; }} return 0; }}",
            "if (0) {} else ".repeat(2000)
        ),
        format!("int main() {{ return root{}; }}", ".field".repeat(2000)),
        format!(
            "int main() {{ int a[1] = {}0{}; return 0; }}",
            "{".repeat(2000),
            "}".repeat(2000)
        ),
        format!(
            "typedef int{} Deep; int main() {{ return 0; }}",
            "*".repeat(2000)
        ),
        format!(
            "typedef int Deep{}; int main() {{ return 0; }}",
            "[1]".repeat(2000)
        ),
    ];
}

#[test]
fn frontend_apis_report_excessive_syntax_expression_and_type_nesting() {
    for source in cases() {
        let error = crust::check(&source).unwrap_err();
        assert!(error.message.contains("nesting limit exceeded"), "{error}");
        let error = crust::analyze(&source).unwrap_err();
        assert!(error.message.contains("nesting limit exceeded"), "{error}");
        let error = crust::compile(&source).unwrap_err();
        assert!(error.message.contains("nesting limit exceeded"), "{error}");
        assert!(error.span.start <= source.len());
    }
    let source = format!(
        "int main() {{ return {}1{}; }}",
        "(".repeat(16),
        ")".repeat(16)
    );
    crust::check(&source).unwrap();
    let source = format!(
        "int main() {{ int n = 0; {}return n; }}",
        "n += 1; ".repeat(5000)
    );
    crust::check(&source).unwrap();
}

#[test]
fn command_line_reports_limits_without_aborting_its_process() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/nesting-tests")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&directory).unwrap();
    for (index, source) in cases().iter().enumerate() {
        let path = directory.join(format!("case_{index}.cr"));
        std::fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_crust"))
            .arg("check")
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("nesting limit exceeded"), "{stderr}");
        assert!(!stderr.contains("overflowed its stack"), "{stderr}");
    }
}
