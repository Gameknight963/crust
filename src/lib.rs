mod ast;
mod check;
mod emit_c;
mod parser;
mod types;

pub mod analysis;
pub mod bindings;
pub mod diagnostic;
pub mod lexer;
pub mod lsp;
pub mod native;
pub mod project;
pub mod source;

use diagnostic::Diagnostic;

pub fn analyze(source: &str) -> Result<analysis::Analysis, Diagnostic> {
    return analyze_inner(source, false);
}

pub(crate) fn analyze_completion(source: &str) -> Result<analysis::Analysis, Diagnostic> {
    return analyze_inner(source, true);
}

fn analyze_inner(source: &str, completion: bool) -> Result<analysis::Analysis, Diagnostic> {
    let tokens = lexer::tokenize(source)?;
    let program = parser::parse(tokens)?;
    let checked = if completion {
        check::analyze_completion(program)?
    } else {
        check::check(program)?.into_analysis()
    };
    return Ok(analysis::build(
        &checked,
        vec![analysis::File {
            path: None,
            text: source.to_owned(),
        }],
        |span| analysis::Location { file: 0, span },
    ));
}

pub fn check(source: &str) -> Result<(), Diagnostic> {
    let tokens = lexer::tokenize(source)?;
    let program = parser::parse(tokens)?;
    check::check(program)?;
    return Ok(());
}

pub fn compile(source: &str) -> Result<String, Diagnostic> {
    let tokens = lexer::tokenize(source)?;
    let program = parser::parse(tokens)?;
    let checked = check::check(program)?;
    return Ok(emit_c::generate(&checked));
}

#[cfg(test)]
mod tests {
    use super::{check, compile};

    #[test]
    fn checks_forward_calls_recursion_and_both_returning_branches() {
        check("int main(void) { return recurse(3); } int recurse(int n) { if (n == 0) { return 0; } else { return recurse(n - 1); } }").unwrap();
    }

    #[test]
    fn rejects_invalid_names_types_calls_and_returns_before_emitting_c() {
        let cases = [
            ("int main(void) { return missing; }", "unknown variable"),
            ("int main(void) { return missing(); }", "unknown function"),
            (
                "int main(void) { print(); return 0; }",
                "expects 1 argument",
            ),
            (
                "int main(void) { int n = print(1); return 0; }",
                "found void",
            ),
            ("int main(void) { return; }", "must return a value"),
            (
                "void f(void) { return 1; } int main(void) { return 0; }",
                "without a value",
            ),
            ("int main(void) { int n = 1; }", "every path"),
            ("int main(void) { if (1) { return 0; } }", "every path"),
            ("int main(void) { while (1) { return 0; } }", "every path"),
            (
                "int main(void) { int n = 0; int n = 1; return n; }",
                "already declared",
            ),
            (
                "int f(int n, int n) { return n; } int main(void) { return 0; }",
                "already declared",
            ),
            (
                "int main(void) { { int n = 1; } return n; }",
                "unknown variable",
            ),
            (
                "int main(void) { int n = n; return n; }",
                "before it is initialized",
            ),
            (
                "int main(void) { int n = 1; { int n = n; } return n; }",
                "before it is initialized",
            ),
            (
                "int main(void) { int print = 0; print(1); return 0; }",
                "not callable",
            ),
            ("int main(void) { return 2147483648; }", "32-bit"),
            (
                "int main(void) { return 0; } int main(void) { return 1; }",
                "already defined",
            ),
            (
                "int print(int n) { return n; } int main(void) { return 0; }",
                "reserved",
            ),
            ("void main(void) {}", "entry point"),
            ("int main(int n) { return n; }", "entry point"),
            ("int other(void) { return 0; }", "entry point"),
        ];
        for (source, expected) in cases {
            let error = compile(source).unwrap_err();
            assert!(error.message.contains(expected), "{source}: {error}");
        }
    }

    #[test]
    fn checks_signed_literal_boundaries_and_contextual_ranges() {
        check("int main(void) { print(-2147483648); return 0; }").unwrap();
        check("int main(void) { print(+2147483648); return 0; }").unwrap();
        assert!(check("int main(void) { int n = 2147483648; return n; }").is_err());
    }

    #[test]
    fn rejects_incomplete_or_unsupported_syntax_with_locations() {
        for source in [
            "int main(void) {",
            "int main(void) { int x; return 0; }",
            "int main(void) { return 1 +; }",
            "int main(void) { if (1) return 0; }",
            "int main(void) { int^ x = 1; return 0; }",
        ] {
            assert!(compile(source).is_err(), "should reject {source}");
        }
        let error = compile("int main(void) {\n return unknown;\n}").unwrap_err();
        assert_eq!((error.span.line, error.span.column), (2, 9));
    }
}
