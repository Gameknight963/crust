use crate::ast::*;
use crate::diagnostic::{Diagnostic, Span};
use crate::lexer::{SpannedToken, Token};

pub fn parse(tokens: Vec<SpannedToken>) -> Result<Program, Diagnostic> {
    let mut parser = Parser {
        tokens,
        position: 0,
    };
    let mut functions = Vec::new();

    while !parser.at(&Token::EndOfFile) {
        functions.push(parser.function()?);
    }

    return Ok(Program { functions });
}

struct Parser {
    tokens: Vec<SpannedToken>,
    position: usize,
}

impl Parser {
    fn function(&mut self) -> Result<Function, Diagnostic> {
        let start = self.current().span;
        let return_type = self.type_name()?;
        let (name, _) = self.identifier()?;
        self.expect(Token::LeftParen, "expected '(' after the function name")?;
        let mut parameters = Vec::new();

        if self.at(&Token::Void) && self.next_at(&Token::RightParen) {
            self.advance();
        } else if !self.at(&Token::RightParen) {
            loop {
                let ty = self.type_name()?;
                if ty == Type::Void {
                    return Err(Diagnostic::new(
                        self.current().span,
                        "a named parameter cannot have type void",
                    ));
                }
                let (name, span) = self.identifier()?;
                parameters.push(Parameter { name, ty, span });
                if !self.consume(&Token::Comma) {
                    break;
                }
            }
        }

        self.expect(Token::RightParen, "expected ')' after the parameters")?;
        let body = self.block()?;
        let span = start.through(body.span);
        return Ok(Function {
            name,
            return_type,
            parameters,
            body,
            span,
        });
    }

    fn type_name(&mut self) -> Result<Type, Diagnostic> {
        let shared = self.consume(&Token::Const);
        let token = self.advance();
        let base = match token.kind {
            Token::Int => Type::Int,
            Token::Void => Type::Void,
            Token::BoxInt => Type::BoxInt,
            _ => {
                return Err(Diagnostic::new(
                    token.span,
                    "expected a type: 'int', 'void', or 'BoxInt'",
                ));
            }
        };
        if self.consume(&Token::Caret) {
            return match base {
                Type::Int => Ok(Type::IntBorrow { mutable: !shared }),
                Type::BoxInt => Ok(Type::BoxBorrow { mutable: !shared }),
                _ => Err(Diagnostic::new(token.span, "void cannot be borrowed")),
            };
        }
        if shared {
            return Err(Diagnostic::new(
                token.span,
                "const currently applies to borrows, such as 'const int^'",
            ));
        }
        return Ok(base);
    }

    fn block(&mut self) -> Result<Block, Diagnostic> {
        let start = self.expect(Token::LeftBrace, "expected '{' to start a block")?;
        let mut statements = Vec::new();
        while !self.at(&Token::RightBrace) {
            if self.at(&Token::EndOfFile) {
                return Err(Diagnostic::new(
                    self.current().span,
                    "expected '}' before end of file",
                ));
            }
            statements.push(self.statement()?);
        }
        let end = self.advance().span;
        return Ok(Block {
            statements,
            span: start.through(end),
        });
    }

    fn statement(&mut self) -> Result<Statement, Diagnostic> {
        let start = self.current().span;

        if self.at(&Token::LeftBrace) {
            let block = self.block()?;
            return Ok(Statement {
                span: block.span,
                kind: StatementKind::Block(block),
            });
        }

        if self.consume(&Token::If) {
            self.expect(Token::LeftParen, "expected '(' after 'if'")?;
            let condition = self.expression(1)?;
            self.expect(Token::RightParen, "expected ')' after the condition")?;
            let then_block = self.block()?;
            let else_block = if self.consume(&Token::Else) {
                Some(self.block()?)
            } else {
                None
            };
            let end = else_block
                .as_ref()
                .map_or(then_block.span, |block| block.span);
            return Ok(Statement {
                span: start.through(end),
                kind: StatementKind::If {
                    condition,
                    then_block,
                    else_block,
                },
            });
        }

        if self.consume(&Token::While) {
            self.expect(Token::LeftParen, "expected '(' after 'while'")?;
            let condition = self.expression(1)?;
            self.expect(Token::RightParen, "expected ')' after the condition")?;
            let body = self.block()?;
            return Ok(Statement {
                span: start.through(body.span),
                kind: StatementKind::While { condition, body },
            });
        }

        let kind;
        if self.at(&Token::Int) || self.at(&Token::BoxInt) || self.at(&Token::Const) {
            let ty = self.type_name()?;
            let (name, _) = self.identifier()?;
            self.expect(
                Token::Equal,
                "local variables need an initializer: 'type name = expression;'",
            )?;
            let initializer = self.expression(1)?;
            kind = StatementKind::Local {
                name,
                ty,
                initializer,
            };
        } else if self.consume(&Token::Return) {
            let value = if self.at(&Token::Semicolon) {
                None
            } else {
                Some(self.expression(1)?)
            };
            kind = StatementKind::Return(value);
        } else if self.at(&Token::Semicolon) {
            kind = StatementKind::Empty;
        } else if matches!(self.current().kind, Token::Identifier(_)) && self.next_at(&Token::Equal)
        {
            let (name, _) = self.identifier()?;
            self.advance(); // =
            let value = self.expression(1)?;
            kind = StatementKind::Assign { name, value };
        } else {
            let expression = self.expression(1)?;
            if self.consume(&Token::Equal) {
                let value = self.expression(1)?;
                if let ExpressionKind::Variable(name) = &expression.kind {
                    kind = StatementKind::Assign {
                        name: name.clone(),
                        value,
                    };
                } else {
                    kind = StatementKind::Store {
                        target: expression,
                        value,
                    };
                }
            } else {
                kind = StatementKind::Expression(expression);
            }
        }

        let end = self.expect(Token::Semicolon, "expected ';' after the statement")?;
        return Ok(Statement {
            kind,
            span: start.through(end),
        });
    }

    // Higher precedence binds more tightly. The recursive call with +1 makes
    // operators at the same precedence left-associative: a - b - c = (a - b) - c.
    fn expression(&mut self, minimum: u8) -> Result<Expression, Diagnostic> {
        let mut left = self.unary()?;
        while let Some((operator, precedence)) = Self::binary_operator(&self.current().kind) {
            if precedence < minimum {
                break;
            }
            self.advance();
            let right = self.expression(precedence + 1)?;
            let span = left.span.through(right.span);
            left = Expression {
                span,
                kind: ExpressionKind::Binary {
                    operator,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            };
        }
        return Ok(left);
    }

    fn unary(&mut self) -> Result<Expression, Diagnostic> {
        let operator = match self.current().kind {
            Token::Plus => Some(UnaryOperator::Plus),
            Token::Minus => Some(UnaryOperator::Negate),
            Token::Bang => Some(UnaryOperator::Not),
            Token::Star => Some(UnaryOperator::Dereference),
            Token::Caret => Some(UnaryOperator::Borrow { mutable: true }),
            _ => None,
        };
        if let Some(mut operator) = operator {
            let start = self.advance().span;
            if matches!(operator, UnaryOperator::Borrow { .. }) && self.consume(&Token::Const) {
                operator = UnaryOperator::Borrow { mutable: false };
            }
            let operand = self.unary()?;
            return Ok(Expression {
                span: start.through(operand.span),
                kind: ExpressionKind::Unary {
                    operator,
                    operand: Box::new(operand),
                },
            });
        }
        return self.primary();
    }

    fn primary(&mut self) -> Result<Expression, Diagnostic> {
        let token = self.advance();
        match token.kind {
            Token::IntegerLiteral(value) => {
                return Ok(Expression {
                    kind: ExpressionKind::Integer(value),
                    span: token.span,
                });
            }
            Token::Identifier(name) => {
                if !self.consume(&Token::LeftParen) {
                    return Ok(Expression {
                        kind: ExpressionKind::Variable(name),
                        span: token.span,
                    });
                }
                let mut arguments = Vec::new();
                if !self.at(&Token::RightParen) {
                    loop {
                        arguments.push(self.expression(1)?);
                        if !self.consume(&Token::Comma) {
                            break;
                        }
                    }
                }
                let end = self.expect(Token::RightParen, "expected ')' after the arguments")?;
                return Ok(Expression {
                    span: token.span.through(end),
                    kind: ExpressionKind::Call { name, arguments },
                });
            }
            Token::LeftParen => {
                let mut expression = self.expression(1)?;
                let end = self.expect(Token::RightParen, "expected ')' after the expression")?;
                expression.span = token.span.through(end);
                return Ok(expression);
            }
            _ => {
                return Err(Diagnostic::new(
                    token.span,
                    format!("expected an expression, found {:?}", token.kind),
                ));
            }
        }
    }

    fn binary_operator(token: &Token) -> Option<(BinaryOperator, u8)> {
        return match token {
            Token::PipePipe => Some((BinaryOperator::Or, 1)),
            Token::AmpersandAmpersand => Some((BinaryOperator::And, 2)),
            Token::Pipe => Some((BinaryOperator::BitOr, 3)),
            Token::Caret => Some((BinaryOperator::BitXor, 4)),
            Token::Ampersand => Some((BinaryOperator::BitAnd, 5)),
            Token::EqualEqual => Some((BinaryOperator::Equal, 6)),
            Token::BangEqual => Some((BinaryOperator::NotEqual, 6)),
            Token::Less => Some((BinaryOperator::Less, 7)),
            Token::LessEqual => Some((BinaryOperator::LessEqual, 7)),
            Token::Greater => Some((BinaryOperator::Greater, 7)),
            Token::GreaterEqual => Some((BinaryOperator::GreaterEqual, 7)),
            Token::Plus => Some((BinaryOperator::Add, 8)),
            Token::Minus => Some((BinaryOperator::Subtract, 8)),
            Token::Star => Some((BinaryOperator::Multiply, 9)),
            Token::Slash => Some((BinaryOperator::Divide, 9)),
            Token::Percent => Some((BinaryOperator::Remainder, 9)),
            _ => None,
        };
    }

    fn identifier(&mut self) -> Result<(String, Span), Diagnostic> {
        let token = self.advance();
        return match token.kind {
            Token::Identifier(name) => Ok((name, token.span)),
            _ => Err(Diagnostic::new(token.span, "expected an identifier")),
        };
    }

    fn current(&self) -> &SpannedToken {
        return &self.tokens[self.position];
    }

    fn at(&self, token: &Token) -> bool {
        return &self.current().kind == token;
    }

    fn next_at(&self, token: &Token) -> bool {
        return self
            .tokens
            .get(self.position + 1)
            .is_some_and(|next| &next.kind == token);
    }

    fn advance(&mut self) -> SpannedToken {
        let token = self.current().clone();
        if token.kind != Token::EndOfFile {
            self.position += 1;
        }
        return token;
    }

    fn consume(&mut self, token: &Token) -> bool {
        if self.at(token) {
            self.advance();
            return true;
        }
        return false;
    }

    fn expect(&mut self, token: Token, message: &str) -> Result<Span, Diagnostic> {
        if self.at(&token) {
            return Ok(self.advance().span);
        }
        return Err(Diagnostic::new(self.current().span, message));
    }
}
