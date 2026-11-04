use super::*;

impl Lexer<'_> {
    pub(super) fn number(&mut self, start: Span) -> Result<Token, LexError> {
        let bytes = self.source.as_bytes();
        let mut end = self.offset;
        while let Some(byte) = bytes.get(end).copied() {
            if byte.is_ascii_alphanumeric()
                || byte == b'_'
                || byte == b'.'
                || (matches!(byte, b'+' | b'-')
                    && end > start.start
                    && matches!(bytes[end - 1], b'e' | b'E' | b'p' | b'P'))
            {
                end += 1;
            } else {
                break;
            }
        }
        let text = &self.source[start.start..end];
        let hexadecimal = text.starts_with("0x") || text.starts_with("0X");
        let floating = if hexadecimal {
            text.contains(['.', 'p', 'P'])
        } else {
            !text.starts_with("0b") && !text.starts_with("0B") && text.contains(['.', 'e', 'E'])
        };
        if !floating {
            return self.integer(start);
        }
        let suffix = text
            .chars()
            .last()
            .filter(|character| matches!(character, 'f' | 'F' | 'l' | 'L'));
        let body = if suffix.is_some() {
            &text[..text.len() - 1]
        } else {
            text
        };
        let (mantissa, exponent) = if hexadecimal {
            let Some(position) = body.find(['p', 'P']) else {
                return Err(self.error(
                    start,
                    "a hexadecimal floating literal requires a p exponent",
                ));
            };
            (&body[2..position], Some(&body[position + 1..]))
        } else if let Some(position) = body.find(['e', 'E']) {
            (&body[..position], Some(&body[position + 1..]))
        } else {
            (body, None)
        };
        let radix = if hexadecimal { 16 } else { 10 };
        let mut digits = 0;
        let mut dots = 0;
        for character in mantissa.chars() {
            if character == '.' {
                dots += 1;
            } else if character.is_ascii() && character.is_digit(radix) {
                digits += 1;
            } else {
                return Err(self.error(start, "invalid floating literal"));
            }
        }
        if digits == 0 || dots > 1 {
            return Err(self.error(start, "invalid floating mantissa"));
        }
        if let Some(exponent) = exponent {
            let exponent = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
            if exponent.is_empty() || !exponent.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(self.error(start, "invalid floating exponent"));
            }
        }
        let spelling = text.to_owned();
        while self.offset < end {
            self.advance();
        }
        return Ok(Token::FloatingLiteral { spelling, suffix });
    }
}
