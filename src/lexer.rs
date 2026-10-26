use crate::diagnostic::Diagnostic;
pub use crate::diagnostic::Span;

pub type LexError = Diagnostic;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    // Keywords.
    Int,
    Char,
    Signed,
    Unsigned,
    Short,
    Long,
    Bool,
    True,
    False,
    Void,
    Return,
    If,
    Else,
    While,
    For,
    Break,
    Continue,
    BoxInt,
    Const,
    Struct,
    Union,
    Interface,
    Import,
    Init,
    Destroy,
    Enum,
    Typedef,
    Static,
    Extern,
    Cdecl,
    Stdcall,
    Fastcall,
    Vectorcall,
    Unsafe,
    Private,
    Sizeof,
    Alignof,

    // Tokens carrying data.
    Identifier(String),
    IntegerLiteral(u64),
    StringLiteral(Vec<u8>),

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
    Ellipsis,
    Colon,
    ColonColon,

    // Operators.
    Equal,
    PlusEqual,
    MinusEqual,
    StarEqual,
    SlashEqual,
    PercentEqual,
    AmpersandEqual,
    PipeEqual,
    CaretEqual,
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
    GreaterGreater,
    LessLess,
    LessLessEqual,
    GreaterGreaterEqual,
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
            } else if current == '\'' {
                kind = self.character(start)?;
            } else if current == '"' {
                kind = self.string(start)?;
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
            "char" => Token::Char,
            "signed" => Token::Signed,
            "unsigned" => Token::Unsigned,
            "short" => Token::Short,
            "long" => Token::Long,
            "bool" => Token::Bool,
            "true" => Token::True,
            "false" => Token::False,
            "void" => Token::Void,
            "return" => Token::Return,
            "if" => Token::If,
            "else" => Token::Else,
            "while" => Token::While,
            "for" => Token::For,
            "break" => Token::Break,
            "continue" => Token::Continue,
            "BoxInt" => Token::BoxInt,
            "const" => Token::Const,
            "struct" => Token::Struct,
            "union" => Token::Union,
            "interface" => Token::Interface,
            "import" => Token::Import,
            "init" => Token::Init,
            "destroy" => Token::Destroy,
            "enum" => Token::Enum,
            "typedef" => Token::Typedef,
            "static" => Token::Static,
            "extern" => Token::Extern,
            "__cdecl" => Token::Cdecl,
            "__stdcall" => Token::Stdcall,
            "__fastcall" => Token::Fastcall,
            "__vectorcall" => Token::Vectorcall,
            "unsafe" => Token::Unsafe,
            "private" => Token::Private,
            "sizeof" => Token::Sizeof,
            "alignof" | "_Alignof" => Token::Alignof,
            _ => Token::Identifier(name.to_owned()),
        };
    }

    fn integer(&mut self, start: Span) -> Result<Token, LexError> {
        let mut radix = 10;
        let mut digits_start = start.start;
        if self.peek() == Some('0') {
            radix = match self.peek_next() {
                Some('x' | 'X') => 16,
                Some('b' | 'B') => 2,
                Some('0'..='9') => 8,
                _ => 10,
            };
            if radix == 16 || radix == 2 {
                self.advance();
                self.advance();
                digits_start = self.offset;
            }
        }
        while let Some(character) = self.peek() {
            if !character.is_ascii() || !character.is_digit(radix) {
                break;
            }
            self.advance();
        }

        // Keep malformed literals together rather than splitting off a name.
        if self.peek().is_some_and(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '.'
        }) {
            return Err(self.error(start, "invalid integer digit or unsupported literal suffix"));
        }

        let text = &self.source[digits_start..self.offset];
        if text.is_empty() {
            return Err(self.error(start, "expected digits after integer base prefix"));
        }

        return match u64::from_str_radix(text, radix) {
            Ok(value) => Ok(Token::IntegerLiteral(value)),
            Err(_) => Err(self.error(start, "integer literal exceeds the unsigned 64-bit range")),
        };
    }

    fn character(&mut self, start: Span) -> Result<Token, LexError> {
        self.advance(); // Opening quote.
        let value = match self.peek() {
            Some('\\') => {
                self.advance();
                self.byte_escape(start)?
            }
            Some('\'' | '\n' | '\r') | None => {
                return Err(self.error(start, "expected one character inside character literal"));
            }
            Some(character) if character.is_ascii() => {
                self.advance();
                character as u64
            }
            Some(_) => {
                self.advance();
                return Err(self.error(
                    start,
                    "character literals require an ASCII character or byte escape",
                ));
            }
        };
        if self.peek() != Some('\'') {
            return Err(self.error(start, "expected closing quote after one character"));
        }
        self.advance();
        return Ok(Token::IntegerLiteral(value));
    }

    fn string(&mut self, start: Span) -> Result<Token, LexError> {
        self.advance();
        let mut bytes = Vec::new();
        loop {
            match self.peek() {
                Some('"') => {
                    self.advance();
                    return Ok(Token::StringLiteral(bytes));
                }
                Some('\n' | '\r') | None => {
                    return Err(self.error(start, "unterminated string literal"));
                }
                Some('\\') => {
                    self.advance();
                    bytes.push(self.byte_escape(start)? as u8);
                }
                Some(character) => {
                    let mut buffer = [0u8; 4];
                    bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
                    self.advance();
                }
            }
        }
    }

    fn byte_escape(&mut self, start: Span) -> Result<u64, LexError> {
        let character = self
            .peek()
            .ok_or_else(|| self.error(start, "unterminated escape sequence"))?;
        if character == 'x' || matches!(character, '0'..='7') {
            let hexadecimal = character == 'x';
            let radix = if hexadecimal { 16 } else { 8 };
            if hexadecimal {
                self.advance();
            }
            let mut value = 0u64;
            let mut count = 0;
            while let Some(digit) = self
                .peek()
                .filter(|c| c.is_ascii())
                .and_then(|c| c.to_digit(radix))
            {
                if !hexadecimal && count == 3 {
                    break;
                }
                self.advance();
                count += 1;
                value = value * radix as u64 + digit as u64;
                if value > 255 {
                    return Err(self.error(start, "escape exceeds one byte"));
                }
            }
            if count == 0 {
                return Err(self.error(start, "expected hexadecimal digits after \\x"));
            }
            return Ok(value);
        }
        self.advance();
        return match character {
            'a' => Ok(7),
            'b' => Ok(8),
            'f' => Ok(12),
            'n' => Ok(10),
            'r' => Ok(13),
            't' => Ok(9),
            'v' => Ok(11),
            '\\' | '\'' | '"' | '?' => Ok(character as u64),
            _ => Err(self.error(start, "unknown escape sequence")),
        };
    }

    fn operator_or_punctuation(&mut self, start: Span) -> Result<Token, LexError> {
        if self.source[self.offset..].starts_with("...") {
            for _ in 0..3 {
                self.advance();
            }
            return Ok(Token::Ellipsis);
        }
        if self.peek() == Some('.') && self.peek_next().is_some_and(|next| next.is_ascii_digit()) {
            self.advance();
            return Err(self.error(start, "floating-point literals are not supported yet"));
        }

        // Even unsupported C operators must stay distinct from spaced tokens:
        // x++ is not the same token sequence as x + +y.
        for operator in ["++", "--"] {
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

        for (operator, token) in [
            ("<<=", Token::LessLessEqual),
            (">>=", Token::GreaterGreaterEqual),
        ] {
            if self.source[self.offset..].starts_with(operator) {
                for _ in 0..3 {
                    self.advance();
                }
                return Ok(token);
            }
        }
        // Try longer operators first, so == becomes one token instead of two.
        let pair = match (self.peek(), self.peek_next()) {
            (Some('='), Some('=')) => Some(Token::EqualEqual),
            (Some('!'), Some('=')) => Some(Token::BangEqual),
            (Some('<'), Some('=')) => Some(Token::LessEqual),
            (Some('>'), Some('=')) => Some(Token::GreaterEqual),
            (Some('>'), Some('>')) => Some(Token::GreaterGreater),
            (Some('<'), Some('<')) => Some(Token::LessLess),
            (Some('&'), Some('&')) => Some(Token::AmpersandAmpersand),
            (Some('|'), Some('|')) => Some(Token::PipePipe),
            (Some('-'), Some('>')) => Some(Token::Arrow),
            (Some(':'), Some(':')) => Some(Token::ColonColon),
            (Some('+'), Some('=')) => Some(Token::PlusEqual),
            (Some('-'), Some('=')) => Some(Token::MinusEqual),
            (Some('*'), Some('=')) => Some(Token::StarEqual),
            (Some('/'), Some('=')) => Some(Token::SlashEqual),
            (Some('%'), Some('=')) => Some(Token::PercentEqual),
            (Some('&'), Some('=')) => Some(Token::AmpersandEqual),
            (Some('|'), Some('=')) => Some(Token::PipeEqual),
            (Some('^'), Some('=')) => Some(Token::CaretEqual),
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
    fn keeps_shifts_compound_shifts_and_spaced_angles_distinct() {
        assert_eq!(
            kinds("<< >> <<= >>= < < > >"),
            vec![
                Token::LessLess,
                Token::GreaterGreater,
                Token::LessLessEqual,
                Token::GreaterGreaterEqual,
                Token::Less,
                Token::Less,
                Token::Greater,
                Token::Greater,
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
            kinds("18446744073709551615"),
            vec![Token::IntegerLiteral(u64::MAX), Token::EndOfFile]
        );
        let error = tokenize("18446744073709551616").unwrap_err();
        assert!(error.message.contains("64-bit range"));
    }

    #[test]
    fn strings_keep_utf8_bytes_escapes_and_source_locations() {
        let tokens = tokenize("\"é\\n\"\r\n\"x\"").unwrap();
        assert_eq!(tokens[0].kind, Token::StringLiteral(vec![195, 169, 10]));
        assert_eq!((tokens[0].span.start, tokens[0].span.end), (0, 6));
        assert_eq!((tokens[1].span.line, tokens[1].span.column), (2, 1));
        for source in ["\"unfinished", "\"line\nbreak\"", r#""\q""#, r#""\x100""#] {
            let error = tokenize(&format!("\n  {source}")).unwrap_err();
            assert_eq!((error.span.line, error.span.column), (2, 3));
        }
    }

    #[test]
    fn rejects_unsupported_number_syntax_instead_of_splitting_it() {
        for source in [
            "0x", "0b", "0xg", "0b102", "09", "42u", "1_000", "1.5", ".5", "1e3",
        ] {
            assert!(tokenize(source).is_err(), "should reject {source}");
        }
    }

    #[test]
    fn scans_integer_bases_and_byte_character_literals() {
        assert_eq!(
            kinds(r#"0xff 0XFF 0b1010 0B11 077 '\n' 'A' '\xff' '\377' '\'' '\\'"#),
            vec![
                Token::IntegerLiteral(255),
                Token::IntegerLiteral(255),
                Token::IntegerLiteral(10),
                Token::IntegerLiteral(3),
                Token::IntegerLiteral(63),
                Token::IntegerLiteral(10),
                Token::IntegerLiteral(65),
                Token::IntegerLiteral(255),
                Token::IntegerLiteral(255),
                Token::IntegerLiteral(39),
                Token::IntegerLiteral(92),
                Token::EndOfFile,
            ]
        );
        assert_eq!(
            kinds("0xffffffffffffffff"),
            vec![Token::IntegerLiteral(u64::MAX), Token::EndOfFile]
        );
        for source in [
            "0x10000000000000000",
            "0b11111111111111111111111111111111111111111111111111111111111111111",
        ] {
            assert!(tokenize(source).is_err(), "should reject {source}");
        }
    }

    #[test]
    fn rejects_invalid_characters_and_escape_sequences_at_the_opening_quote() {
        for source in [
            "''", "'ab'", "'a", "'\n'", "'é'", r"'\q'", r"'\x'", r"'\x100'", r"'\400'", r"'\0000'",
        ] {
            let error = tokenize(&format!("\n  {source}")).unwrap_err();
            assert_eq!((error.span.line, error.span.column), (2, 3), "{source}");
        }
        let tokens = tokenize("'a'\r\n'\\n' + 1").unwrap();
        assert_eq!((tokens[1].span.line, tokens[1].span.column), (2, 1));
        assert_eq!((tokens[2].span.line, tokens[2].span.column), (2, 6));
    }

    #[test]
    fn rejects_unsupported_compound_operators_without_confusing_spaced_ones() {
        for source in ["x++", "x--"] {
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
