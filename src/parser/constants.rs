use super::*;

#[derive(Clone, Copy)]
pub(super) struct EnumConstant {
    pub value: i128,
    pub kind: IntType,
    pub span: Span,
}

#[derive(Clone, Copy)]
pub(super) struct IntegerConstant {
    pub value: i128,
    kind: IntType,
}

impl IntegerConstant {
    fn literal(value: i128) -> Self {
        let kind = if IntType::I32.fits(value) {
            IntType::I32
        } else if IntType::I64.fits(value) {
            IntType::I64
        } else {
            IntType::U64
        };
        return Self { value, kind };
    }

    fn convert(self, kind: IntType, evaluate: bool, span: Span) -> Result<Self, Diagnostic> {
        if evaluate && !kind.fits(self.value) {
            return Err(Diagnostic::new(
                span,
                "constant integer conversion out of range",
            ));
        }
        return Ok(Self {
            kind,
            value: if evaluate { self.value } else { 0 },
        });
    }

    fn arithmetic(
        value: i128,
        kind: IntType,
        evaluate: bool,
        span: Span,
    ) -> Result<Self, Diagnostic> {
        let value = if !evaluate {
            0
        } else if kind.signed() {
            if !kind.fits(value) {
                return Err(Diagnostic::new(span, "constant integer overflow"));
            }
            value
        } else {
            value & kind.maximum()
        };
        return Ok(Self { value, kind });
    }
}

impl Parser {
    pub(super) fn global_constant_context<T>(
        &mut self,
        position: usize,
        parse: impl FnOnce(&mut Self) -> Result<T, Diagnostic>,
    ) -> Result<T, Diagnostic> {
        let saved = self.position;
        let greater = self.pending_greater.take();
        let bindings = std::mem::take(&mut self.generics.bindings);
        let declarations = std::mem::take(&mut self.generics.binding_declarations);
        let function = self.generics.current_function.take();
        let owner = self.current_struct.take();
        let scopes = std::mem::replace(&mut self.value_scopes, vec![HashSet::new()]);
        self.position = position;
        let result = parse(self);
        self.position = saved;
        self.pending_greater = greater;
        self.generics.bindings = bindings;
        self.generics.binding_declarations = declarations;
        self.generics.current_function = function;
        self.current_struct = owner;
        self.value_scopes = scopes;
        return result;
    }

    pub(super) fn collect_enum_sources(&mut self) {
        let mut depth = 0usize;
        for position in 0..self.tokens.len().saturating_sub(2) {
            let token = &self.tokens[position];
            if depth == 0
                && token.kind == Token::Enum
                && matches!(
                    self.tokens[position + 2].kind,
                    Token::LeftBrace | Token::Colon | Token::Equal
                )
                && let Token::Identifier(name) = &self.tokens[position + 1].kind
            {
                let id = self
                    .enum_names
                    .iter()
                    .position(|(existing, _)| existing == name)
                    .unwrap();
                self.enum_sources.entry(id).or_insert(position);
                let mut cursor = position + 2;
                while cursor < self.tokens.len()
                    && !matches!(
                        self.tokens[cursor].kind,
                        Token::LeftBrace | Token::Semicolon | Token::EndOfFile
                    )
                {
                    cursor += 1;
                }
                if self
                    .tokens
                    .get(cursor)
                    .is_some_and(|token| token.kind == Token::LeftBrace)
                {
                    cursor += 1;
                    let mut nested = 0usize;
                    let mut member = true;
                    while let Some(token) = self.tokens.get(cursor) {
                        if nested == 0 && matches!(token.kind, Token::RightBrace | Token::EndOfFile)
                        {
                            break;
                        }
                        if member && let Token::Identifier(name) = &token.kind {
                            self.enumerator_owners.entry(name.clone()).or_insert(id);
                        }
                        member = nested == 0 && token.kind == Token::Comma;
                        match token.kind {
                            Token::LeftParen | Token::LeftBracket | Token::LeftBrace => nested += 1,
                            Token::RightParen | Token::RightBracket | Token::RightBrace => {
                                nested = nested.saturating_sub(1)
                            }
                            _ => {}
                        }
                        cursor += 1;
                    }
                }
            }
            match token.kind {
                Token::LeftBrace => depth += 1,
                Token::RightBrace => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }

    fn evaluate_enum(&mut self, id: usize, name: &str) -> Result<(), Diagnostic> {
        let Some(position) = self.enum_sources.get(&id).copied() else {
            return Ok(());
        };
        if self.evaluating_enums.len() >= 64 {
            return Err(Diagnostic::new(
                self.tokens[position].span,
                "enum constant dependency depth exceeds 64",
            ));
        }
        self.global_constant_context(position, |parser| parser.enum_definition_until(Some(name)))?;
        return Ok(());
    }

    fn constant_enum_representation(
        &mut self,
        id: usize,
        span: Span,
    ) -> Result<IntType, Diagnostic> {
        if let Some(kind) = self.enum_representations.get(&id) {
            return Ok(*kind);
        }
        let Some(position) = self.enum_sources.get(&id).copied() else {
            return Err(Diagnostic::new(
                span,
                "constant casts require a declared enum representation",
            ));
        };
        let ty = self.global_constant_context(position + 2, |parser| {
            if parser.consume(&Token::Equal) {
                parser.advance();
            }
            if parser.consume(&Token::Colon) {
                return parser.type_name();
            }
            return Ok(Type::Integer(IntType::I32));
        })?;
        let Type::Integer(kind) = ty else {
            return Err(Diagnostic::new(
                span,
                "an enum underlying type must be an integer",
            ));
        };
        self.enum_representations.insert(id, kind);
        return Ok(kind);
    }

    pub(super) fn integer_constant(
        &mut self,
        expression: &Expression,
        evaluate: bool,
    ) -> Result<IntegerConstant, Diagnostic> {
        let value = self.integer_constant_inner(expression, evaluate)?;
        if !value.kind.fits(value.value) {
            return Err(Diagnostic::new(
                expression.span,
                "integer literal exceeds the 64-bit range",
            ));
        }
        self.type_annotations
            .push((expression.span, Type::Integer(value.kind)));
        return Ok(value);
    }

    fn constant_enumerator(
        &mut self,
        id: Option<usize>,
        name: &str,
        span: Span,
    ) -> Result<IntegerConstant, Diagnostic> {
        if id.is_none()
            && self
                .value_scopes
                .iter()
                .rev()
                .any(|scope| scope.contains(name))
        {
            return Err(Diagnostic::new(
                span,
                "a local value cannot be used in an integer constant expression",
            ));
        }
        let owner = id.or_else(|| self.enumerator_owners.get(name).copied());
        if let Some(owner) = owner
            && !self.enum_constants.contains_key(&(owner, name.to_owned()))
            && !self.evaluating_enums.contains(&owner)
        {
            self.evaluate_enum(owner, name)?;
        }
        let value = self
            .enum_constants
            .iter()
            .find_map(|((owner, member), value)| {
                return (member == name && id.is_none_or(|id| *owner == id)).then_some(*value);
            })
            .ok_or_else(|| {
                Diagnostic::new(
                    span,
                    "a constant expression requires a previously declared enum value",
                )
            })?;
        self.type_references.push(TypeReference {
            span,
            declaration: Some(TypeDeclaration::Source(value.span)),
        });
        return Ok(IntegerConstant {
            value: value.value,
            kind: value.kind,
        });
    }

    fn integer_constant_inner(
        &mut self,
        expression: &Expression,
        evaluate: bool,
    ) -> Result<IntegerConstant, Diagnostic> {
        let span = expression.span;
        match &expression.kind {
            ExpressionKind::Conditional {
                condition,
                then_value,
                else_value,
            } => {
                let condition = self.integer_constant(condition, evaluate)?;
                let yes = condition.value != 0;
                let then_value = self.integer_constant(then_value, evaluate && yes)?;
                let else_value = self.integer_constant(else_value, evaluate && !yes)?;
                let kind = then_value.kind.common(else_value.kind);
                return if yes { then_value } else { else_value }.convert(kind, evaluate, span);
            }
            ExpressionKind::Integer(value) => {
                return Ok(IntegerConstant::literal(i128::from(*value)));
            }
            ExpressionKind::Boolean(value) => {
                return Ok(IntegerConstant::literal(i128::from(*value)));
            }
            ExpressionKind::Variable { name, name_span } => {
                return self.constant_enumerator(None, name, *name_span);
            }
            ExpressionKind::EnumValue {
                id,
                name,
                name_span,
            } => return self.constant_enumerator(Some(*id), name, *name_span),
            ExpressionKind::Cast { ty, operand } => {
                let value = self.integer_constant(operand, evaluate)?;
                let kind = match ty {
                    Type::Integer(kind) => *kind,
                    Type::Bool => {
                        return Ok(IntegerConstant::literal(i128::from(value.value != 0)));
                    }
                    Type::Enum(id) => self.constant_enum_representation(*id, span)?,
                    _ => {
                        return Err(Diagnostic::new(
                            span,
                            "constant casts require an integer, bool, or enum type",
                        ));
                    }
                };
                return value.convert(kind, evaluate, span);
            }
            ExpressionKind::Unary { operator, operand } => {
                if let ExpressionKind::Integer(value) = operand.kind {
                    if *operator == UnaryOperator::Plus {
                        return Ok(IntegerConstant::literal(i128::from(value)));
                    }
                    if *operator == UnaryOperator::Negate {
                        return Ok(IntegerConstant::literal(-i128::from(value)));
                    }
                }
                let value = self.integer_constant(operand, evaluate)?;
                let kind = value.kind.promoted();
                let result = match operator {
                    UnaryOperator::Plus => value.value,
                    UnaryOperator::Negate => -value.value,
                    UnaryOperator::BitNot => !value.value,
                    UnaryOperator::Not => {
                        return Ok(IntegerConstant::literal(i128::from(value.value == 0)));
                    }
                    _ => {
                        return Err(Diagnostic::new(
                            span,
                            "this operator is not allowed in an integer constant expression",
                        ));
                    }
                };
                return IntegerConstant::arithmetic(result, kind, evaluate, span);
            }
            ExpressionKind::Binary {
                operator,
                left,
                right,
            } => {
                let left = self.integer_constant(left, evaluate)?;
                let right_evaluated = evaluate
                    && match operator {
                        BinaryOperator::And => left.value != 0,
                        BinaryOperator::Or => left.value == 0,
                        _ => true,
                    };
                let right = self.integer_constant(right, right_evaluated)?;
                if *operator == BinaryOperator::And {
                    return Ok(IntegerConstant::literal(i128::from(
                        left.value != 0 && right.value != 0,
                    )));
                }
                if *operator == BinaryOperator::Or {
                    return Ok(IntegerConstant::literal(i128::from(
                        left.value != 0 || right.value != 0,
                    )));
                }
                if operator.is_shift() {
                    let kind = left.kind.promoted();
                    if !evaluate {
                        return IntegerConstant::arithmetic(0, kind, false, span);
                    }
                    if right.value < 0 || right.value >= i128::from(kind.bits()) {
                        return Err(Diagnostic::new(span, "constant shift count out of range"));
                    }
                    let count = right.value as u32;
                    let result = if *operator == BinaryOperator::ShiftLeft {
                        left.value << count
                    } else {
                        left.value >> count
                    };
                    return IntegerConstant::arithmetic(result, kind, true, span);
                }
                let kind = left.kind.common(right.kind);
                let a = left.convert(kind, evaluate, span)?.value;
                let b = right.convert(kind, evaluate, span)?.value;
                if !evaluate {
                    return IntegerConstant::arithmetic(0, kind, false, span);
                }
                if matches!(operator, BinaryOperator::Divide | BinaryOperator::Remainder) {
                    if b == 0 {
                        return Err(Diagnostic::new(
                            span,
                            "division or remainder by zero in a constant expression",
                        ));
                    }
                    if kind.signed() && a == kind.minimum() && b == -1 {
                        return Err(Diagnostic::new(span, "constant integer overflow"));
                    }
                }
                let result = match operator {
                    BinaryOperator::Add => a + b,
                    BinaryOperator::Subtract => a - b,
                    BinaryOperator::Multiply => a.wrapping_mul(b),
                    BinaryOperator::Divide => a / b,
                    BinaryOperator::Remainder => a % b,
                    BinaryOperator::BitAnd => a & b,
                    BinaryOperator::BitOr => a | b,
                    BinaryOperator::BitXor => a ^ b,
                    BinaryOperator::Equal => {
                        return Ok(IntegerConstant::literal(i128::from(a == b)));
                    }
                    BinaryOperator::NotEqual => {
                        return Ok(IntegerConstant::literal(i128::from(a != b)));
                    }
                    BinaryOperator::Less => return Ok(IntegerConstant::literal(i128::from(a < b))),
                    BinaryOperator::LessEqual => {
                        return Ok(IntegerConstant::literal(i128::from(a <= b)));
                    }
                    BinaryOperator::Greater => {
                        return Ok(IntegerConstant::literal(i128::from(a > b)));
                    }
                    BinaryOperator::GreaterEqual => {
                        return Ok(IntegerConstant::literal(i128::from(a >= b)));
                    }
                    _ => unreachable!("logical and shift constants handled before arithmetic"),
                };
                return IntegerConstant::arithmetic(result, kind, true, span);
            }
            _ => {
                return Err(Diagnostic::new(
                    span,
                    "expected an integer constant expression",
                ));
            }
        }
    }
}
