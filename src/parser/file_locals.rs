use super::*;

impl Parser {
    fn file_key(&self, span: Span) -> usize {
        return self.file_ends.partition_point(|end| *end < span.start);
    }

    pub(super) fn file_function_name(&self, name: &str, span: Span) -> String {
        return self
            .file_locals
            .get(&(self.file_key(span), name.to_owned()))
            .cloned()
            .unwrap_or_else(|| name.to_owned());
    }

    pub(super) fn collect_file_locals(&mut self) -> Result<(), Diagnostic> {
        self.file_ends = self
            .tokens
            .iter()
            .filter(|token| token.kind == Token::EndOfFile)
            .map(|token| token.span.start)
            .collect();
        let mut braces = 0usize;
        let mut parens = 0usize;
        let mut local = false;
        let mut declarations = HashMap::new();
        for (position, token) in self.tokens.iter().enumerate() {
            if braces == 0 && parens == 0 {
                if token.kind == Token::Static
                    && !self
                        .tokens
                        .get(position + 1)
                        .is_some_and(|token| token.kind == Token::Interface)
                {
                    local = true;
                }
                if let Token::Identifier(name) = &token.kind {
                    let generic =
                        generics::parameters(&self.tokens, position + 1).map(|(_, after)| after);
                    let after = generic.unwrap_or(position + 1);
                    if self
                        .tokens
                        .get(after)
                        .is_some_and(|token| token.kind == Token::LeftParen)
                    {
                        let key = (self.file_key(token.span), name.clone());
                        if let Some(previous) = declarations.insert(key.clone(), local)
                            && previous != local
                        {
                            return Err(Diagnostic::new(
                                token.span,
                                "function has conflicting static and public declarations",
                            ));
                        }
                        if local {
                            if name == "main" {
                                return Err(Diagnostic::new(token.span, "main cannot be static"));
                            }
                            self.file_locals
                                .insert(key.clone(), format!("!{}_{}", key.0, name));
                        }
                    }
                }
            }
            match token.kind {
                Token::LeftBrace => braces += 1,
                Token::RightBrace => {
                    braces = braces.saturating_sub(1);
                    if braces == 0 {
                        local = false;
                    }
                }
                Token::LeftParen => parens += 1,
                Token::RightParen => parens = parens.saturating_sub(1),
                Token::Semicolon | Token::EndOfFile if braces == 0 => local = false,
                _ => {}
            }
        }
        return Ok(());
    }
}
