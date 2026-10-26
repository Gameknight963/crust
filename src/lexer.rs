use crate::diagnostic::Diagnostic;
pub use crate::diagnostic::Span;

pub type LexError = Diagnostic;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    // Keywords.
    Int,
    Void,
    Return,
    If,
    Else,
    While,
    BoxInt,
    Const,
    Struct,
    Typedef,
    Static,

    // Tokens carrying data.
    Identifier(String),
    IntegerLiteral(i64),

    // Punctuation.
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Semicolon,
    Comma,
    Dot,
    Colon,
    ColonColon,

    // Operators.
    Equal,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    Less,
    Greater,
    Ampersand,
    Pipe,
    Caret,
    Tilde,
    EqualEqual,
    BangEqual,
    LessEqual,
    GreaterEqual,
    AmpersandAmpersand,
    PipePipe,
    Arrow,

    EndOfFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpannedToken {
    pub kind: Token,
    pub span: Span,
}

pub fn tokenize(source: &str) -> Result<Vec<SpannedToken>, LexError> {
    let mut lexer = Lexer {
        source,
        offset: 0,
        line: 1,
        column: 1,
    };

    return lexer.scan();
}

// 'source says that the lexer borrows its input for as long as it scans it.
// Tokens own their identifier text, so they can outlive this temporary lexer.
struct Lexer<'source> {
    source: &'source str,
    offset: usize,
    line: usize,
    column: usize,
}

impl Lexer<'_> {
    fn scan(&mut self) -> Result<Vec<SpannedToken>, LexError> {
        let mut tokens: Vec<SpannedToken> = Vec::new();

        while let Some(current) = self.peek() {
            if current.is_ascii_whitespace() {
                self.advance();
                continue;
            }

            let start = self.location();

            if current == '/' && self.peek_next() == Some('/') {
                self.skip_line_comment();
                continue;
            }

            if current == '/' && self.peek_next() == Some('*') {
                // ? returns the error immediately if the comment never closes.
                self.skip_block_comment(start)?;
                continue;
            }

            let kind;

            if current.is_ascii_alphabetic() || current == '_' {
                kind = self.identifier(start);
            } else if current.is_ascii_digit() {
                kind = self.integer(start)?;
            } else {
                kind = self.operator_or_punctuation(start)?;
            }

            tokens.push(SpannedToken {
                kind,
                span: self.finish_span(start),
            });
        }

        tokens.push(SpannedToken {
            kind: Token::EndOfFile,
            span: self.location(),
        });

        return Ok(tokens);
    }

    fn identifier(&mut self, start: Span) -> Token {
        while let Some(character) = self.peek() {
            if !character.is_ascii_alphanumeric() && character != '_' {
                break;
            }
            self.advance();
        }

        let name = &self.source[start.start..self.offset];

        return match name {
            "int" => Token::Int,
            "void" => Token::Void,
            "return" => Token::Return,
            "if" => Token::If,
            "else" => Token::Else,
            "while" => Token::While,
            "BoxInt" => Token::BoxInt,
            "const" => Token::Const,
            "struct" => Token::Struct,
            "typedef" => Token::Typedef,
            "static" => Token::Static,
            _ => Token::Identifier(name.to_owned()),
        };
    }

    fn integer(&mut self, start: Span) -> Result<Token, LexError> {
        while let Some(character) = self.peek() {
            if !character.is_ascii_digit() {
                break;
            }
            self.advance();
        }

        // Don't split unsupported literals such as 0xff or 42u into an integer
        // and an identifier. Floating-point syntax is also reserved for later.
        if self.peek().is_some_and(|character| {
            character.is_ascii_alphabetic() || character == '_' || character == '.'
        }) {
            return Err(self.error(start, "only unsuffixed decimal integers are supported"));
        }

        let text = &self.source[start.start..self.offset];

        // Avoid silently treating a C-style octal literal as decimal.
        if text.len() > 1 && text.starts_with('0') {
            return Err(self.error(start, "integers with leading zeros are not supported yet"));
        }

        return match text.parse::<i64>() {
            Ok(value) => Ok(Token::IntegerLiteral(value)),
            Err(_) => Err(self.error(start, "integer literal exceeds the signed 64-bit range")),
        };
    }

    fn operator_or_punctuation(&mut self, start: Span) -> Result<Token, LexError> {
        if self.peek() == Some('.') && self.peek_next().is_some_and(|next| next.is_ascii_digit()) {
            self.advance();
            return Err(self.error(start, "floating-point literals are not supported yet"));
        }

        // Even unsupported C operators must stay distinct from spaced tokens:
        // x++ is not the same token sequence as x + +y.
        for operator in [
            "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<", ">>", "...",
        ] {
            if self.source[self.offset..].starts_with(operator) {
                for _ in operator.chars() {
                    self.advance();
                }
                return Err(self.error(
                    start,
                    &format!("operator {operator:?} is not supported yet"),
                ));
            }
        }

        // Try longer operators first, so == becomes one token instead of two.
        let pair = match (self.peek(), self.peek_next()) {
            (Some('='), Some('=')) => Some(Token::EqualEqual),
            (Some('!'), Some('=')) => Some(Token::BangEqual),
            (Some('<'), Some('=')) => Some(Token::LessEqual),
            (Some('>'), Some('=')) => Some(Token::GreaterEqual),
            (Some('&'), Some('&')) => Some(Token::AmpersandAmpersand),
            (Some('|'), Some('|')) => Some(Token::PipePipe),
            (Some('-'), Some('>')) => Some(Token::Arrow),
            (Some(':'), Some(':')) => Some(Token::ColonColon),
            _ => None,
        };

        if let Some(token) = pair {
            self.advance();
            self.advance();
            return Ok(token);
        }

        let token = match self.peek() {
            Some('(') => Token::LeftParen,
            Some(')') => Token::RightParen,
            Some('{') => Token::LeftBrace,
            Some('}') => Token::RightBrace,
            Some('[') => Token::LeftBracket,
            Some(']') => Token::RightBracket,
            Some(';') => Token::Semicolon,
            Some(',') => Token::Comma,
            Some('.') => Token::Dot,
            Some(':') => Token::Colon,
            Some('=') => Token::Equal,
            Some('+') => Token::Plus,
            Some('-') => Token::Minus,
            Some('*') => Token::Star,
            Some('/') => Token::Slash,
            Some('%') => Token::Percent,
            Some('!') => Token::Bang,
            Some('<') => Token::Less,
            Some('>') => Token::Greater,
            Some('&') => Token::Ampersand,
            Some('|') => Token::Pipe,
            Some('^') => Token::Caret,
            Some('~') => Token::Tilde,
            Some(character) => {
                self.advance();
                return Err(self.error(start, &format!("unexpected character {character:?}")));
            }
            None => {
                return Err(self.error(start, "expected an operator or punctuation"));
            }
        };

        self.advance();
        return Ok(token);
    }

    fn skip_line_comment(&mut self) {
        while let Some(character) = self.peek() {
            if character == '\n' || character == '\r' {
                break;
            }
            self.advance();
        }
    }

    fn skip_block_comment(&mut self, start: Span) -> Result<(), LexError> {
        self.advance(); // /
        self.advance(); // *

        while self.peek().is_some() {
            if self.peek() == Some('*') && self.peek_next() == Some('/') {
                self.advance();
                self.advance();
                return Ok(());
            }
            self.advance();
        }

        return Err(self.error(start, "unterminated block comment"));
    }

    fn peek(&self) -> Option<char> {
        return self.source[self.offset..].chars().next();
    }

    fn peek_next(&self) -> Option<char> {
        return self.source[self.offset..].chars().nth(1);
    }

    fn advance(&mut self) {
        if let Some(character) = self.peek() {
            // Treat CRLF as one newline, and also accept LF and bare CR.
            if character == '\r'
                || (character == '\n' && !self.source[..self.offset].ends_with('\r'))
            {
                self.line += 1;
                self.column = 1;
            } else if character != '\n' {
                self.column += 1;
            }

            // Rust strings use UTF-8, so one character can occupy several bytes.
            self.offset += character.len_utf8();
        }
    }

    fn location(&self) -> Span {
        return Span {
            start: self.offset,
            end: self.offset,
            line: self.line,
            column: self.column,
        };
    }

    fn finish_span(&self, start: Span) -> Span {
        return Span {
            end: self.offset,
            ..start
        };
    }

    fn error(&self, start: Span, message: &str) -> LexError {
        return LexError {
            message: message.to_owned(),
            span: self.finish_span(start),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::{Span, Token, tokenize};

    fn kinds(source: &str) -> Vec<Token> {
        return tokenize(source)
            .expect("source should tokenize")
            .into_iter()
            .map(|token| token.kind)
            .collect();
    }

    #[test]
    fn tokenizes_the_example_program() {
        assert_eq!(
            kinds(include_str!("../examples/hello.cr")),
            vec![
                Token::Int,
                Token::Identifier(String::from("main")),
                Token::LeftParen,
                Token::Void,
                Token::RightParen,
                Token::LeftBrace,
                Token::Int,
                Token::Identifier(String::from("answer")),
                Token::Equal,
                Token::IntegerLiteral(42),
                Token::Semicolon,
                Token::Identifier(String::from("print")),
                Token::LeftParen,
                Token::Identifier(String::from("answer")),
                Token::RightParen,
                Token::Semicolon,
                Token::Return,
                Token::IntegerLiteral(0),
                Token::Semicolon,
                Token::RightBrace,
                Token::EndOfFile,
            ]
        );
    }

    #[test]
    fn keywords_must_match_the_entire_identifier() {
        assert_eq!(
            kinds("int integer void voided return return_value _value2"),
            vec![
                Token::Int,
                Token::Identifier(String::from("integer")),
                Token::Void,
                Token::Identifier(String::from("voided")),
                Token::Return,
                Token::Identifier(String::from("return_value")),
                Token::Identifier(String::from("_value2")),
                Token::EndOfFile,
            ]
        );
    }

    #[test]
    fn prefers_longer_operators_and_keeps_minus_separate_from_numbers() {
        assert_eq!(
            kinds("a==42&&b!=-1||c<=2&&d>=3->x"),
            vec![
                Token::Identifier(String::from("a")),
                Token::EqualEqual,
                Token::IntegerLiteral(42),
                Token::AmpersandAmpersand,
                Token::Identifier(String::from("b")),
                Token::BangEqual,
                Token::Minus,
                Token::IntegerLiteral(1),
                Token::PipePipe,
                Token::Identifier(String::from("c")),
                Token::LessEqual,
                Token::IntegerLiteral(2),
                Token::AmpersandAmpersand,
                Token::Identifier(String::from("d")),
                Token::GreaterEqual,
                Token::IntegerLiteral(3),
                Token::Arrow,
                Token::Identifier(String::from("x")),
                Token::EndOfFile,
            ]
        );
    }

    #[test]
    fn comments_separate_tokens_without_hiding_division() {
        assert_eq!(
            kinds("int/* comment */value = 8 / 2; // until EOF"),
            vec![
                Token::Int,
                Token::Identifier(String::from("value")),
                Token::Equal,
                Token::IntegerLiteral(8),
                Token::Slash,
                Token::IntegerLiteral(2),
                Token::Semicolon,
                Token::EndOfFile,
            ]
        );
    }

    #[test]
    fn preserves_byte_spans_and_character_columns_after_unicode_comments() {
        let source = "/* é */ int\r\n  return\rvoid\n";
        let tokens = tokenize(source).unwrap();

        assert_eq!(
            tokens[0].span,
            Span {
                start: 9,
                end: 12,
                line: 1,
                column: 9
            }
        );
        assert_eq!(
            tokens[1].span,
            Span {
                start: 16,
                end: 22,
                line: 2,
                column: 3
            }
        );
        assert_eq!(
            tokens[2].span,
            Span {
                start: 23,
                end: 27,
                line: 3,
                column: 1
            }
        );
        assert_eq!(
            tokens[3].span,
            Span {
                start: 28,
                end: 28,
                line: 4,
                column: 1
            }
        );

        for token in tokens {
            assert!(source.get(token.span.start..token.span.end).is_some());
        }
    }

    #[test]
    fn empty_input_and_comment_only_input_have_one_eof_token() {
        for source in ["", " \t\r\n", "// comment", "/**/", "/* outer /* inner */"] {
            assert_eq!(kinds(source), vec![Token::EndOfFile]);
        }
    }

    #[test]
    fn reports_unterminated_comments_at_the_opening_delimiter() {
        let error = tokenize("\n  /* never closed").unwrap_err();
        assert_eq!(error.message, "unterminated block comment");
        assert_eq!((error.span.line, error.span.column), (2, 3));
    }

    #[test]
    fn checks_integer_range() {
        assert_eq!(
            kinds("9223372036854775807"),
            vec![Token::IntegerLiteral(i64::MAX), Token::EndOfFile]
        );
        let error = tokenize("9223372036854775808").unwrap_err();
        assert!(error.message.contains("64-bit range"));
    }

    #[test]
    fn rejects_unsupported_number_syntax_instead_of_splitting_it() {
        for source in ["0xff", "0b10", "42u", "1_000", "1.5", ".5", "1e3", "077"] {
            assert!(tokenize(source).is_err(), "should reject {source}");
        }
    }

    #[test]
    fn rejects_unsupported_compound_operators_without_confusing_spaced_ones() {
        for source in ["x++", "x--", "x+=1", "x<<=2", "x>>1", "..."] {
            let error = tokenize(source).unwrap_err();
            assert!(error.message.contains("not supported yet"));
        }

        assert_eq!(
            kinds("x + +y"),
            vec![
                Token::Identifier(String::from("x")),
                Token::Plus,
                Token::Plus,
                Token::Identifier(String::from("y")),
                Token::EndOfFile,
            ]
        );
    }

    #[test]
    fn unexpected_unicode_is_an_error_instead_of_a_string_index_panic() {
        let error = tokenize("\n  é").unwrap_err();
        assert_eq!((error.span.line, error.span.column), (2, 3));
        assert_eq!((error.span.start, error.span.end), (3, 5));
        assert!(error.message.contains("unexpected character"));
    }
}
