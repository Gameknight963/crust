use crate::ast::*;
use crate::diagnostic::{Diagnostic, Span};
use crate::lexer::{SpannedToken, Token};
use std::collections::{HashMap, HashSet};

pub fn parse(tokens: Vec<SpannedToken>) -> Result<Program, Diagnostic> {
    let mut parser = Parser {
        tokens,
        position: 0,
        struct_names: Vec::new(),
        current_struct: None,
        methods: Vec::new(),
        aliases: HashMap::new(),
        resolved_aliases: HashMap::new(),
        resolving_aliases: HashSet::new(),
        value_scopes: vec![HashSet::new()],
        arrays: Vec::new(),
    };
    parser.collect_type_names()?;
    let mut functions = Vec::new();
    let mut structs = Vec::new();

    while !parser.at(&Token::EndOfFile) {
        if parser.consume(&Token::Typedef) {
            let ty = parser.type_name()?;
            parser.identifier()?;
            parser.array_suffix(ty)?;
            parser.expect(Token::Semicolon, "expected ';' after typedef")?;
        } else if parser.at(&Token::Struct)
            && parser
                .tokens
                .get(parser.position + 2)
                .is_some_and(|token| token.kind == Token::LeftBrace)
        {
            structs.push(parser.struct_definition()?);
        } else {
            functions.push(parser.function()?);
        }
    }

    functions.append(&mut parser.methods);
    return Ok(Program {
        arrays: parser.arrays,
        functions,
        structs,
        struct_names: parser.struct_names,
    });
}

struct Parser {
    arrays: Vec<ArrayType>,
    tokens: Vec<SpannedToken>,
    position: usize,
    struct_names: Vec<(String, Span)>,
    current_struct: Option<usize>,
    methods: Vec<Function>,
    aliases: HashMap<String, Alias>,
    resolved_aliases: HashMap<String, Type>,
    resolving_aliases: HashSet<String>,
    value_scopes: Vec<HashSet<String>>,
}

#[derive(Clone, Copy)]
struct Alias {
    position: usize,
    name_position: usize,
    span: Span,
}

impl Parser {
    // Collect top-level type declarations first so forward uses have the same
    // meaning throughout the file. Type aliases expand before the checker.
    fn collect_type_names(&mut self) -> Result<(), Diagnostic> {
        let mut depth = 0usize;
        let mut position = 0;
        while position < self.tokens.len() {
            let token = self.tokens[position].clone();
            if depth == 0
                && token.kind == Token::Struct
                && self
                    .tokens
                    .get(position + 2)
                    .is_some_and(|token| token.kind == Token::LeftBrace)
                && let Some(SpannedToken {
                    kind: Token::Identifier(name),
                    span,
                }) = self.tokens.get(position + 1).cloned()
            {
                if name == "Self" || name == "this" {
                    return Err(Diagnostic::new(span, "Self and this are reserved names"));
                }
                self.struct_id(name, span);
            }
            if depth == 0 && token.kind == Token::Typedef {
                let start = position + 1;
                let mut end = start;
                while let Some(token) = self.tokens.get(end) {
                    if matches!(
                        token.kind,
                        Token::Semicolon | Token::EndOfFile | Token::LeftBrace
                    ) {
                        break;
                    }
                    end += 1;
                }
                if !self
                    .tokens
                    .get(end)
                    .is_some_and(|token| token.kind == Token::Semicolon)
                {
                    return Err(Diagnostic::new(token.span, "expected 'typedef type Name;'"));
                }
                let name_position = (start..end)
                    .rfind(|&index| matches!(self.tokens[index].kind, Token::Identifier(_)))
                    .unwrap_or(start);
                if name_position <= start {
                    return Err(Diagnostic::new(
                        token.span,
                        "a typedef needs both a type and an alias name",
                    ));
                }
                let SpannedToken {
                    kind: Token::Identifier(name),
                    span,
                } = self.tokens[name_position].clone()
                else {
                    return Err(Diagnostic::new(token.span, "a typedef needs an alias name"));
                };
                if name == "Self" || name == "this" {
                    return Err(Diagnostic::new(span, "Self and this are reserved names"));
                }
                if self
                    .aliases
                    .insert(
                        name.clone(),
                        Alias {
                            position: start,
                            name_position,
                            span,
                        },
                    )
                    .is_some()
                {
                    return Err(Diagnostic::new(
                        span,
                        format!("typedef '{name}' is already defined"),
                    ));
                }
                position = end + 1;
                continue;
            }
            match token.kind {
                Token::LeftBrace => depth += 1,
                Token::RightBrace => depth = depth.saturating_sub(1),
                _ => {}
            }
            position += 1;
        }
        let mut aliases: Vec<_> = self
            .aliases
            .iter()
            .map(|(name, alias)| (alias.position, name.clone()))
            .collect();
        aliases.sort_by_key(|entry| entry.0);
        for (_, name) in aliases {
            self.alias_type(&name)?;
        }
        return Ok(());
    }

    fn alias_type(&mut self, name: &str) -> Result<Type, Diagnostic> {
        if let Some(ty) = self.resolved_aliases.get(name) {
            return Ok(*ty);
        }
        let alias = self.aliases[name];
        if !self.resolving_aliases.insert(name.to_owned()) {
            return Err(Diagnostic::new(
                alias.span,
                format!("cyclic typedef involving '{name}'"),
            ));
        }
        let previous = self.position;
        self.position = alias.position;
        let ty = self.type_name()?;
        if self.position != alias.name_position {
            return Err(Diagnostic::new(
                self.current().span,
                "expected 'typedef type Name;'",
            ));
        }
        self.identifier()?;
        let ty = self.array_suffix(ty)?;
        self.position = previous;
        self.resolving_aliases.remove(name);
        self.resolved_aliases.insert(name.to_owned(), ty);
        return Ok(ty);
    }

    fn named_type(&mut self, name: &str, span: Span) -> Result<Type, Diagnostic> {
        if name == "Self" {
            return self
                .current_struct
                .map(Type::Struct)
                .ok_or_else(|| Diagnostic::new(span, "Self is only available inside a struct"));
        }
        if self.aliases.contains_key(name) {
            return self.alias_type(name);
        }
        if let Some(kind) = IntType::named(name) {
            return Ok(Type::Integer(kind));
        }
        if let Some(id) = self.struct_names.iter().position(|entry| entry.0 == name) {
            return Ok(Type::Struct(id));
        }
        return Err(Diagnostic::new(span, format!("unknown type '{name}'")));
    }

    fn known_type(&self, name: &str) -> bool {
        return name == "Self"
            || IntType::named(name).is_some()
            || self.aliases.contains_key(name)
            || self.struct_names.iter().any(|entry| entry.0 == name);
    }

    fn type_in_expression(&self) -> bool {
        return match &self.current().kind {
            Token::Int
            | Token::Bool
            | Token::Void
            | Token::BoxInt
            | Token::Struct
            | Token::Const => true,
            Token::Identifier(name) => {
                self.known_type(name)
                    && !self
                        .value_scopes
                        .iter()
                        .rev()
                        .any(|scope| scope.contains(name))
            }
            _ => false,
        };
    }

    fn local_type_name(&self) -> bool {
        if !matches!(&self.current().kind, Token::Identifier(name) if self.known_type(name)) {
            return false;
        }
        if matches!(
            self.tokens.get(self.position + 1).map(|token| &token.kind),
            Some(Token::Identifier(_))
        ) {
            return true;
        }
        // A type name may also be a value name. Keep `Count ^ other` available
        // as an XOR expression, while recognizing initialized borrow locals.
        return self.next_at(&Token::Caret)
            && matches!(
                self.tokens.get(self.position + 2).map(|token| &token.kind),
                Some(Token::Identifier(_))
            )
            && self
                .tokens
                .get(self.position + 3)
                .is_some_and(|token| token.kind == Token::Equal);
    }

    fn struct_id(&mut self, name: String, span: Span) -> usize {
        if let Some(id) = self.struct_names.iter().position(|entry| entry.0 == name) {
            return id;
        }
        let id = self.struct_names.len();
        self.struct_names.push((name, span));
        return id;
    }

    fn struct_definition(&mut self) -> Result<StructDefinition, Diagnostic> {
        let start = self.advance().span;
        let (name, span) = self.identifier()?;
        let id = self.struct_id(name, span);
        self.current_struct = Some(id);
        self.expect(Token::LeftBrace, "expected '{' after the struct name")?;
        let mut fields = Vec::new();
        let mut has_destructor = false;
        while !self.at(&Token::RightBrace) {
            if self.at(&Token::EndOfFile) {
                return Err(Diagnostic::new(
                    self.current().span,
                    "expected '}' after the struct fields",
                ));
            }
            if self.at(&Token::Tilde) {
                let start = self.advance().span;
                let (name, name_span) = self.identifier()?;
                if name != self.struct_names[id].0 {
                    return Err(Diagnostic::new(
                        name_span,
                        "the destructor name must match its enclosing struct",
                    ));
                }
                if has_destructor {
                    return Err(Diagnostic::new(
                        start,
                        "a struct can only define one destructor",
                    ));
                }
                self.expect(Token::LeftParen, "expected '(' after the destructor name")?;
                self.expect(
                    Token::RightParen,
                    "destructors cannot have explicit parameters",
                )?;
                if self.at(&Token::Const) {
                    return Err(Diagnostic::new(
                        self.current().span,
                        "destructors always have an exclusive receiver and cannot be const",
                    ));
                }
                let body = self.block()?;
                self.methods.push(Function {
                    name: destructor_name(id),
                    return_type: Type::Void,
                    parameters: vec![Parameter {
                        name: String::from("this"),
                        ty: Type::Borrow {
                            pointee: Pointee::Struct(id),
                            mutable: true,
                        },
                        span: start,
                    }],
                    span: start.through(body.span),
                    body,
                });
                has_destructor = true;
                continue;
            }
            let is_static = self.consume(&Token::Static);
            let ty = self.type_name()?;
            let (name, span) = self.identifier()?;
            if self.at(&Token::LeftParen) {
                let key = if is_static {
                    static_method_name(id, &name)
                } else {
                    method_name(id, &name)
                };
                let method = self.function_body(ty, key, span, Some(id), is_static)?;
                self.methods.push(method);
                continue;
            }
            if is_static {
                return Err(Diagnostic::new(span, "static currently applies to methods"));
            }
            let ty = self.array_suffix(ty)?;
            self.expect(Token::Semicolon, "expected ';' after the field")?;
            fields.push(Field { name, ty, span });
        }
        self.advance();
        self.current_struct = None;
        let end = self.expect(Token::Semicolon, "expected ';' after the struct definition")?;
        return Ok(StructDefinition {
            id,
            fields,
            has_destructor,
            span: start.through(end),
        });
    }

    fn function(&mut self) -> Result<Function, Diagnostic> {
        let start = self.current().span;
        let return_type = self.type_name()?;
        let (name, _) = self.identifier()?;
        return self.function_body(return_type, name, start, None, false);
    }

    fn function_body(
        &mut self,
        return_type: Type,
        name: String,
        start: Span,
        owner: Option<usize>,
        is_static: bool,
    ) -> Result<Function, Diagnostic> {
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
                let ty = self.array_suffix(ty)?;
                parameters.push(Parameter { name, ty, span });
                if !self.consume(&Token::Comma) {
                    break;
                }
            }
        }

        self.expect(Token::RightParen, "expected ')' after the parameters")?;
        let shared = self.consume(&Token::Const);
        if is_static && shared {
            return Err(Diagnostic::new(
                start,
                "a static method has no receiver to qualify as const",
            ));
        }
        if let Some(id) = owner.filter(|_| !is_static) {
            if parameters
                .first()
                .is_some_and(|parameter| parameter.name == "this")
            {
                if shared || parameters[0].ty != Type::Struct(id) {
                    return Err(Diagnostic::new(
                        start,
                        "an explicit receiver must be 'Self this' and cannot have a const method qualifier",
                    ));
                }
            } else {
                parameters.insert(
                    0,
                    Parameter {
                        name: String::from("this"),
                        ty: Type::Borrow {
                            pointee: Pointee::Struct(id),
                            mutable: !shared,
                        },
                        span: start,
                    },
                );
            }
        } else if shared {
            return Err(Diagnostic::new(
                start,
                "only instance methods can have a const method qualifier",
            ));
        }
        self.value_scopes.push(
            parameters
                .iter()
                .map(|parameter| parameter.name.clone())
                .collect(),
        );
        let body = self.block()?;
        self.value_scopes.pop();
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
            Token::Int => Type::Integer(IntType::I32),
            Token::Bool => Type::Bool,
            Token::Void => Type::Void,
            Token::BoxInt => Type::BoxInt,
            Token::Struct => {
                let (name, span) = self.identifier()?;
                Type::Struct(self.struct_id(name, span))
            }
            Token::Identifier(ref name) => self.named_type(name, token.span)?,
            _ => {
                return Err(Diagnostic::new(
                    token.span,
                    "expected a type: 'int', 'bool', 'void', 'BoxInt', or a type name",
                ));
            }
        };
        let base = self.array_suffix(base)?;
        if self.consume(&Token::Caret) {
            return base
                .borrowed(!shared)
                .ok_or_else(|| Diagnostic::new(token.span, "void cannot be borrowed"));
        }
        if shared {
            return Err(Diagnostic::new(
                token.span,
                "const currently applies to borrows, such as 'const int^'",
            ));
        }
        return Ok(base);
    }

    fn array_suffix(&mut self, mut element: Type) -> Result<Type, Diagnostic> {
        let mut dimensions = Vec::new();
        while self.consume(&Token::LeftBracket) {
            let token = self.advance();
            let Token::IntegerLiteral(length) = token.kind else {
                return Err(Diagnostic::new(
                    token.span,
                    "array length must be a positive integer literal",
                ));
            };
            let length = usize::try_from(length)
                .ok()
                .filter(|length| *length > 0)
                .ok_or_else(|| {
                    Diagnostic::new(token.span, "array length must fit size_t and be positive")
                })?;
            self.expect(Token::RightBracket, "expected ']' after array length")?;
            dimensions.push((length, token.span));
        }
        for (length, span) in dimensions.into_iter().rev() {
            if element == Type::Void || element.is_borrow() {
                return Err(Diagnostic::new(span, "array elements must be values"));
            }
            let id = if let Some(id) = self
                .arrays
                .iter()
                .position(|array| array.element == element && array.length == length)
            {
                id
            } else {
                let id = self.arrays.len();
                self.arrays.push(ArrayType {
                    element,
                    length,
                    span,
                });
                id
            };
            element = Type::Array(id);
        }
        return Ok(element);
    }

    fn block(&mut self) -> Result<Block, Diagnostic> {
        let start = self.expect(Token::LeftBrace, "expected '{' to start a block")?;
        self.value_scopes.push(HashSet::new());
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
        self.value_scopes.pop();
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
                kind: StatementKind::While {
                    condition,
                    body,
                    step: None,
                },
            });
        }

        if self.consume(&Token::For) {
            self.expect(Token::LeftParen, "expected '(' after 'for'")?;
            self.value_scopes.push(HashSet::new());
            let initializer = self.statement()?;
            if !matches!(
                initializer.kind,
                StatementKind::Local { .. }
                    | StatementKind::Assign { .. }
                    | StatementKind::Store { .. }
                    | StatementKind::CompoundAssign { .. }
                    | StatementKind::Expression(_)
                    | StatementKind::Empty
            ) {
                return Err(Diagnostic::new(
                    initializer.span,
                    "a for initializer must be a declaration, assignment, or expression",
                ));
            }
            let condition = if self.at(&Token::Semicolon) {
                Expression {
                    span: self.current().span,
                    kind: ExpressionKind::Integer(1),
                }
            } else {
                self.expression(1)?
            };
            self.expect(Token::Semicolon, "expected ';' after the for condition")?;
            let step_start = self.current().span;
            let step = if self.at(&Token::RightParen) {
                StatementKind::Empty
            } else {
                self.assignment_or_expression()?
            };
            let step_end = self.expect(Token::RightParen, "expected ')' after the for step")?;
            let body = self.block()?;
            self.value_scopes.pop();
            let span = start.through(body.span);
            let loop_statement = Statement {
                span,
                kind: StatementKind::While {
                    condition,
                    body,
                    step: Some(Box::new(Statement {
                        span: step_start.through(step_end),
                        kind: step,
                    })),
                },
            };
            return Ok(Statement {
                span,
                kind: StatementKind::Block(Block {
                    span,
                    statements: vec![initializer, loop_statement],
                }),
            });
        }

        let kind;
        if self.at(&Token::Int)
            || self.at(&Token::Bool)
            || self.at(&Token::BoxInt)
            || self.at(&Token::Const)
            || self.at(&Token::Struct)
            || self.local_type_name()
        {
            let ty = self.type_name()?;
            if ty == Type::Void {
                return Err(Diagnostic::new(
                    start,
                    "a local variable cannot have type void",
                ));
            }
            let (name, _) = self.identifier()?;
            let ty = self.array_suffix(ty)?;
            self.value_scopes.last_mut().unwrap().insert(name.clone());
            self.expect(
                Token::Equal,
                "local variables need an initializer: 'type name = expression;'",
            )?;
            let initializer = self.initializer()?;
            kind = StatementKind::Local {
                name,
                ty,
                initializer,
            };
        } else if self.consume(&Token::Break) {
            kind = StatementKind::Break;
        } else if self.consume(&Token::Continue) {
            kind = StatementKind::Continue;
        } else if self.consume(&Token::Return) {
            let value = if self.at(&Token::Semicolon) {
                None
            } else {
                Some(self.expression(1)?)
            };
            kind = StatementKind::Return(value);
        } else if self.at(&Token::Semicolon) {
            kind = StatementKind::Empty;
        } else {
            kind = self.assignment_or_expression()?;
        }

        let end = self.expect(Token::Semicolon, "expected ';' after the statement")?;
        return Ok(Statement {
            kind,
            span: start.through(end),
        });
    }

    fn assignment_or_expression(&mut self) -> Result<StatementKind, Diagnostic> {
        let target = self.expression(1)?;
        if self.consume(&Token::Equal) {
            let value = self.initializer()?;
            if let ExpressionKind::Variable(name) = target.kind {
                return Ok(StatementKind::Assign { name, value });
            }
            return Ok(StatementKind::Store { target, value });
        }
        let operator = match self.current().kind {
            Token::PlusEqual => Some(BinaryOperator::Add),
            Token::MinusEqual => Some(BinaryOperator::Subtract),
            Token::StarEqual => Some(BinaryOperator::Multiply),
            Token::SlashEqual => Some(BinaryOperator::Divide),
            Token::PercentEqual => Some(BinaryOperator::Remainder),
            Token::AmpersandEqual => Some(BinaryOperator::BitAnd),
            Token::PipeEqual => Some(BinaryOperator::BitOr),
            Token::CaretEqual => Some(BinaryOperator::BitXor),
            _ => None,
        };
        if let Some(operator) = operator {
            self.advance();
            let value = self.expression(1)?;
            return Ok(StatementKind::CompoundAssign {
                target,
                operator,
                value,
            });
        }
        return Ok(StatementKind::Expression(target));
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
        let start = self.current().span;
        if self.consume(&Token::Alignof) {
            self.expect(Token::LeftParen, "expected '(' after alignof")?;
            let ty = self.type_name()?;
            let end = self.expect(Token::RightParen, "expected ')' after the type")?;
            return Ok(Expression {
                span: start.through(end),
                kind: ExpressionKind::Alignof(ty),
            });
        }
        if self.consume(&Token::Sizeof) {
            let saved = self.position;
            if self.consume(&Token::LeftParen) && self.type_in_expression() {
                let ty = self.type_name()?;
                let end = self.expect(Token::RightParen, "expected ')' after the type")?;
                return Ok(Expression {
                    span: start.through(end),
                    kind: ExpressionKind::SizeofType(ty),
                });
            }
            self.position = saved;
            let operand = self.unary()?;
            return Ok(Expression {
                span: start.through(operand.span),
                kind: ExpressionKind::SizeofValue(Box::new(operand)),
            });
        }
        if self.at(&Token::LeftParen) {
            let saved = self.position;
            self.advance();
            if self.type_in_expression() {
                let ty = self.type_name()?;
                self.expect(Token::RightParen, "expected ')' after the cast type")?;
                if !self.at(&Token::LeftBrace) {
                    let operand = self.unary()?;
                    return Ok(Expression {
                        span: start.through(operand.span),
                        kind: ExpressionKind::Cast {
                            ty,
                            operand: Box::new(operand),
                        },
                    });
                }
            }
            self.position = saved;
        }
        let operator = match self.current().kind {
            Token::Plus => Some(UnaryOperator::Plus),
            Token::Minus => Some(UnaryOperator::Negate),
            Token::Bang => Some(UnaryOperator::Not),
            Token::Tilde => Some(UnaryOperator::BitNot),
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
        return self.postfix();
    }

    fn postfix(&mut self) -> Result<Expression, Diagnostic> {
        let mut expression = self.primary()?;
        loop {
            if self.consume(&Token::LeftBracket) {
                let index = self.expression(1)?;
                let end = self.expect(Token::RightBracket, "expected ']' after index")?;
                expression = Expression {
                    span: expression.span.through(end),
                    kind: ExpressionKind::Index {
                        base: Box::new(expression),
                        index: Box::new(index),
                    },
                };
                continue;
            }
            let arrow = self.consume(&Token::Arrow);
            if !arrow && !self.consume(&Token::Dot) {
                break;
            }
            if self.at(&Token::Tilde) {
                return Err(Diagnostic::new(
                    self.current().span,
                    "destructors cannot be called explicitly",
                ));
            }
            let (name, span) = self.identifier()?;
            if arrow {
                expression = Expression {
                    span: expression.span,
                    kind: ExpressionKind::Unary {
                        operator: UnaryOperator::Dereference,
                        operand: Box::new(expression),
                    },
                };
            }
            if self.consume(&Token::LeftParen) {
                let (arguments, end) = self.arguments()?;
                expression = Expression {
                    span: expression.span.through(end),
                    kind: ExpressionKind::MethodCall {
                        receiver: Box::new(expression),
                        name,
                        arguments,
                    },
                };
                continue;
            }
            expression = Expression {
                span: expression.span.through(span),
                kind: ExpressionKind::Field {
                    base: Box::new(expression),
                    name,
                },
            };
        }
        return Ok(expression);
    }

    fn initializer(&mut self) -> Result<Expression, Diagnostic> {
        if self.at(&Token::LeftBrace) {
            if !self.next_at(&Token::Dot) && !self.next_at(&Token::RightBrace) {
                let start = self.advance().span;
                let mut elements = Vec::new();
                while !self.at(&Token::RightBrace) {
                    elements.push(self.initializer()?);
                    if !self.consume(&Token::Comma) {
                        break;
                    }
                }
                let end = self.expect(Token::RightBrace, "expected '}' after array initializer")?;
                return Ok(Expression {
                    span: start.through(end),
                    kind: ExpressionKind::ArrayLiteral { id: None, elements },
                });
            }
            return self.struct_literal(None);
        }
        return self.expression(1);
    }

    fn arguments(&mut self) -> Result<(Vec<Expression>, Span), Diagnostic> {
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
        return Ok((arguments, end));
    }

    fn struct_literal(&mut self, id: Option<usize>) -> Result<Expression, Diagnostic> {
        let start = self.expect(
            Token::LeftBrace,
            "expected '{' to start a struct initializer",
        )?;
        let mut fields = Vec::new();
        while !self.at(&Token::RightBrace) {
            self.expect(
                Token::Dot,
                "struct initializers need named fields: '.field = value'",
            )?;
            let (name, span) = self.identifier()?;
            self.expect(Token::Equal, "expected '=' after the field name")?;
            let value = self.initializer()?;
            fields.push(FieldInitializer {
                name,
                span: span.through(value.span),
                value,
            });
            if !self.consume(&Token::Comma) {
                break;
            }
        }
        let end = self.expect(
            Token::RightBrace,
            "expected '}' after the struct initializer",
        )?;
        return Ok(Expression {
            span: start.through(end),
            kind: ExpressionKind::StructLiteral { id, fields },
        });
    }

    fn primary(&mut self) -> Result<Expression, Diagnostic> {
        let token = self.advance();
        match token.kind {
            Token::True | Token::False => {
                return Ok(Expression {
                    span: token.span,
                    kind: ExpressionKind::Boolean(token.kind == Token::True),
                });
            }
            Token::IntegerLiteral(value) => {
                return Ok(Expression {
                    kind: ExpressionKind::Integer(value),
                    span: token.span,
                });
            }
            Token::Identifier(name) => {
                if self.consume(&Token::ColonColon) {
                    let Type::Struct(id) = self.named_type(&name, token.span)? else {
                        return Err(Diagnostic::new(
                            token.span,
                            "static calls require a struct type",
                        ));
                    };
                    let (name, _) = self.identifier()?;
                    self.expect(
                        Token::LeftParen,
                        "expected '(' after the static method name",
                    )?;
                    let (arguments, end) = self.arguments()?;
                    return Ok(Expression {
                        span: token.span.through(end),
                        kind: ExpressionKind::StaticCall {
                            id,
                            name,
                            arguments,
                        },
                    });
                }
                if !self.consume(&Token::LeftParen) {
                    return Ok(Expression {
                        kind: ExpressionKind::Variable(name),
                        span: token.span,
                    });
                }
                let (arguments, end) = self.arguments()?;
                return Ok(Expression {
                    span: token.span.through(end),
                    kind: ExpressionKind::Call { name, arguments },
                });
            }
            Token::LeftParen => {
                if self.type_in_expression() {
                    let ty = self.type_name()?;
                    self.expect(
                        Token::RightParen,
                        "expected ')' after the compound literal type",
                    )?;
                    let mut literal = match ty {
                        Type::Struct(id) => self.struct_literal(Some(id))?,
                        Type::Array(id) => {
                            let mut literal = self.initializer()?;
                            if let ExpressionKind::ArrayLiteral { id: literal_id, .. } =
                                &mut literal.kind
                            {
                                *literal_id = Some(id);
                            } else {
                                return Err(Diagnostic::new(
                                    literal.span,
                                    "expected array elements inside braces",
                                ));
                            }
                            literal
                        }
                        _ => {
                            return Err(Diagnostic::new(
                                token.span,
                                "a compound literal needs a struct or array type",
                            ));
                        }
                    };
                    literal.span = token.span.through(literal.span);
                    return Ok(literal);
                }
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
