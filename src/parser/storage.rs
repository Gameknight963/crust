use super::*;

type StorageInitializer = (Option<Expression>, Vec<(Span, i128)>);

pub(super) fn link_name(name: &str) -> String {
    return format!("crust_global_{}", name.replace('!', "local_"));
}

fn bind_static_name(expression: &mut Expression, source: &str, global: &str) {
    match &mut expression.kind {
        ExpressionKind::Variable { name, .. } if name == source => *name = global.to_owned(),
        ExpressionKind::Unary { operand, .. }
        | ExpressionKind::Cast { operand, .. }
        | ExpressionKind::SizeofValue(operand) => bind_static_name(operand, source, global),
        ExpressionKind::Field { base, .. } => bind_static_name(base, source, global),
        ExpressionKind::Index { base, index } => {
            bind_static_name(base, source, global);
            bind_static_name(index, source, global);
        }
        ExpressionKind::Binary { left, right, .. } => {
            bind_static_name(left, source, global);
            bind_static_name(right, source, global);
        }
        ExpressionKind::Conditional {
            condition,
            then_value,
            else_value,
        } => {
            bind_static_name(condition, source, global);
            bind_static_name(then_value, source, global);
            bind_static_name(else_value, source, global);
        }
        ExpressionKind::ArrayLiteral { elements, .. } => {
            for element in elements {
                bind_static_name(element, source, global);
            }
        }
        ExpressionKind::StructLiteral { fields, .. } => {
            for field in fields {
                bind_static_name(&mut field.value, source, global);
            }
        }
        _ => {}
    }
}

pub(super) fn initializer_functions(globals: &[ExternalVariable], functions: &mut Vec<Function>) {
    for global in globals {
        if let Some(initializer) = &global.initializer {
            functions.push(Function {
                name: format!("?init_{}", global.name),
                name_span: global.name_span,
                span: global.span,
                return_type: Type::Void,
                parameters: Vec::new(),
                body: Block {
                    span: global.span,
                    statements: vec![Statement {
                        span: global.span,
                        kind: StatementKind::Local {
                            name: "$init".to_owned(),
                            name_span: global.name_span,
                            declaration_span: global.span,
                            ty: global.ty,
                            initializer: initializer.clone(),
                        },
                    }],
                },
                dll_import: false,
                calling_convention: CallingConvention::C,
                return_borrows: None,
                link_name: None,
                variadic: false,
                requires_unsafe: true,
                external: false,
            });
        }
    }
}

impl Parser {
    pub(super) fn storage_initializer(&mut self) -> Result<StorageInitializer, Diagnostic> {
        if !self.consume(&Token::Equal) {
            return Ok((None, Vec::new()));
        }
        let expression = self.initializer()?;
        let mut constants = Vec::new();
        self.storage_constants(&expression, &mut constants)?;
        return Ok((Some(expression), constants));
    }

    fn integer_storage_expression(&self, expression: &Expression) -> bool {
        return match &expression.kind {
            ExpressionKind::Integer(_)
            | ExpressionKind::TypedInteger { .. }
            | ExpressionKind::Boolean(_)
            | ExpressionKind::EnumValue { .. } => true,
            ExpressionKind::Variable { name, .. } => {
                self.enumerator_owners.contains_key(name)
                    && !self.value_scopes.iter().any(|scope| scope.contains(name))
            }
            ExpressionKind::Unary {
                operator:
                    UnaryOperator::Plus
                    | UnaryOperator::Negate
                    | UnaryOperator::Not
                    | UnaryOperator::BitNot,
                operand,
            } => self.integer_storage_expression(operand),
            ExpressionKind::Binary { left, right, .. } => {
                self.integer_storage_expression(left) && self.integer_storage_expression(right)
            }
            ExpressionKind::Conditional {
                condition,
                then_value,
                else_value,
            } => {
                self.integer_storage_expression(condition)
                    && self.integer_storage_expression(then_value)
                    && self.integer_storage_expression(else_value)
            }
            ExpressionKind::Cast { ty, operand } => {
                !matches!(ty, Type::Float(_) | Type::Pointer(_))
                    && self.integer_storage_expression(operand)
            }
            _ => false,
        };
    }

    fn storage_constants(
        &mut self,
        expression: &Expression,
        constants: &mut Vec<(Span, i128)>,
    ) -> Result<(), Diagnostic> {
        if self.integer_storage_expression(expression) {
            constants.push((
                expression.span,
                self.integer_constant(expression, true)?.value,
            ));
            return Ok(());
        }
        match &expression.kind {
            ExpressionKind::StringLiteral { .. } => {}
            ExpressionKind::ArrayLiteral { elements, .. } => {
                for element in elements {
                    self.storage_constants(element, constants)?;
                }
            }
            ExpressionKind::StructLiteral { fields, .. } => {
                for field in fields {
                    self.storage_constants(&field.value, constants)?;
                }
            }
            ExpressionKind::Floating { .. }
            | ExpressionKind::Variable { .. }
            | ExpressionKind::StaticReference { .. }
            | ExpressionKind::SizeofType(_)
            | ExpressionKind::SizeofValue(_)
            | ExpressionKind::Alignof(_) => {}
            ExpressionKind::Unary {
                operator: UnaryOperator::Address,
                operand,
            } => self.storage_address_constants(operand, constants)?,
            ExpressionKind::Unary {
                operator: UnaryOperator::Plus | UnaryOperator::Negate | UnaryOperator::Not,
                operand,
            }
            | ExpressionKind::Cast { operand, .. } => self.storage_constants(operand, constants)?,
            ExpressionKind::Binary { left, right, .. } => {
                self.storage_constants(left, constants)?;
                self.storage_constants(right, constants)?;
            }
            ExpressionKind::Conditional {
                condition,
                then_value,
                else_value,
            } => {
                self.storage_constants(condition, constants)?;
                self.storage_constants(then_value, constants)?;
                self.storage_constants(else_value, constants)?;
            }
            _ => {
                return Err(Diagnostic::new(
                    expression.span,
                    "static storage requires a constant initializer",
                ));
            }
        }
        return Ok(());
    }

    fn storage_address_constants(
        &mut self,
        expression: &Expression,
        constants: &mut Vec<(Span, i128)>,
    ) -> Result<(), Diagnostic> {
        match &expression.kind {
            ExpressionKind::Variable { .. } => {}
            ExpressionKind::Field { base, .. } => {
                self.storage_address_constants(base, constants)?
            }
            ExpressionKind::Index { base, index } => {
                self.storage_address_constants(base, constants)?;
                constants.push((index.span, self.integer_constant(index, true)?.value));
            }
            _ => {
                return Err(Diagnostic::new(
                    expression.span,
                    "a static address must name a global object or field",
                ));
            }
        }
        return Ok(());
    }

    pub(super) fn static_local(&mut self) -> Result<Statement, Diagnostic> {
        let start = self.advance().span;
        let (ty, qualifiers) = self.qualified_type_name(true)?;
        let (name, name_span) = self.identifier()?;
        if name == "this" {
            return Err(Diagnostic::new(
                name_span,
                "this is reserved for an instance receiver",
            ));
        }
        let ty = self.array_suffix(ty)?;
        self.value_scopes.last_mut().unwrap().insert(name.clone());
        let (mut initializer, constant_values) = self.storage_initializer()?;
        let end = self.expect(Token::Semicolon, "expected ';' after static variable")?;
        let global = format!("!scope_{}_{}", name_span.start, name);
        if let Some(initializer) = &mut initializer {
            bind_static_name(initializer, &name, &global);
        }
        self.static_globals.push(ExternalVariable {
            external: false,
            function_local: true,
            initializer,
            constant_values,
            dll_import: false,
            name: global.clone(),
            name_span,
            ty,
            readonly: qualifiers.readonly,
            volatile: qualifiers.volatile,
            link_name: link_name(&global),
            span: start.through(end),
        });
        return Ok(Statement {
            span: start.through(end),
            kind: StatementKind::StaticLocal {
                name,
                name_span,
                global,
                ty,
            },
        });
    }
}
