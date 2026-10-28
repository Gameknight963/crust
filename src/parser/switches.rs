use super::*;

impl Parser {
    pub(super) fn switch_statement(&mut self) -> Result<Statement, Diagnostic> {
        let start = self.advance().span;
        self.expect(Token::LeftParen, "expected '(' after switch")?;
        let value = self.expression(1)?;
        self.expect(Token::RightParen, "expected ')' after switch value")?;
        self.expect(Token::LeftBrace, "expected '{' before switch cases")?;
        let mut cases = Vec::new();
        while !self.at(&Token::RightBrace) {
            let token = self.advance();
            let label = match token.kind {
                Token::Identifier(name) if name == "case" => {
                    if matches!(self.current().kind, Token::Identifier(_))
                        && self.next_at(&Token::Colon)
                    {
                        let (name, span) = self.identifier()?;
                        Some(CaseLabel::Named { name, span })
                    } else {
                        let expression = self.expression(1)?;
                        let value = self.integer_constant(&expression, true)?.value;
                        Some(CaseLabel::Constant { expression, value })
                    }
                }
                Token::Identifier(name) if name == "default" => None,
                _ => return Err(Diagnostic::new(token.span, "expected case or default")),
            };
            let begin = self.expect(Token::Colon, "expected ':' after switch label")?;
            self.value_scopes.push(HashSet::new());
            let mut statements = Vec::new();
            while !self.at(&Token::RightBrace)
                && !matches!(&self.current().kind, Token::Identifier(name) if name == "case" || name == "default")
            {
                statements.push(self.statement()?);
            }
            self.value_scopes.pop();
            let end = statements.last().map_or(begin, |statement| statement.span);
            cases.push(SwitchCase {
                label,
                body: Block {
                    statements,
                    span: begin.through(end),
                },
            });
        }
        let end = self.advance().span;
        return Ok(Statement {
            kind: StatementKind::Switch { value, cases },
            span: start.through(end),
        });
    }
}
