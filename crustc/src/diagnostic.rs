use std::fmt;

// Offsets are UTF-8 byte offsets. Lines and character columns are one-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
}

impl Span {
    pub fn through(self, last: Span) -> Span {
        return Span {
            end: last.end,
            ..self
        };
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub span: Span,
}

impl Diagnostic {
    pub fn new(span: Span, message: impl Into<String>) -> Diagnostic {
        return Diagnostic {
            span,
            message: message.into(),
        };
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(
            formatter,
            "{}:{}: {}",
            self.span.line, self.span.column, self.message
        );
    }
}

impl std::error::Error for Diagnostic {}
