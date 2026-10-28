use ::crust::diagnostic::Diagnostic;
#[allow(unused_imports)]
pub use ::crust::*;

pub fn fixture(source: &str) -> String {
    if source.contains("Box<") && !source.contains("struct Box") {
        return format!("{source}\n{}", include_str!("box.cr"));
    }
    return source.to_owned();
}

#[allow(dead_code)]
pub fn compile(source: &str) -> Result<String, Diagnostic> {
    return ::crust::compile(&fixture(source));
}
#[allow(dead_code)]
pub fn compile_with_kind(source: &str, kind: ProgramKind) -> Result<String, Diagnostic> {
    return ::crust::compile_with_kind(&fixture(source), kind);
}
#[allow(dead_code)]
pub fn check(source: &str) -> Result<(), Diagnostic> {
    return ::crust::check(&fixture(source));
}
#[allow(dead_code)]
pub fn check_with_kind(source: &str, kind: ProgramKind) -> Result<(), Diagnostic> {
    return ::crust::check_with_kind(&fixture(source), kind);
}
#[allow(dead_code)]
pub fn analyze(source: &str) -> Result<analysis::Analysis, Diagnostic> {
    return ::crust::analyze(&fixture(source));
}
#[allow(dead_code)]
pub fn analyze_with_kind(
    source: &str,
    kind: ProgramKind,
) -> Result<analysis::Analysis, Diagnostic> {
    return ::crust::analyze_with_kind(&fixture(source), kind);
}
#[allow(dead_code)]
pub fn emit_header(source: &str, kind: ProgramKind) -> Result<String, Diagnostic> {
    return ::crust::emit_header(&fixture(source), kind);
}
