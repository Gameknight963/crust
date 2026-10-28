use super::*;

impl<'checked> Emitter<'checked> {
    pub(super) fn global_definitions(&mut self, checked: &'checked CheckedProgram) {
        for global in &checked.program.globals {
            if global.external {
                continue;
            }
            let value = if let Some(initializer) = &global.initializer {
                let analysis = &checked.analyses[&format!("?init_{}", global.name)];
                self.expression_types = &analysis.expression_types;
                self.conversions = &analysis.conversions;
                self.constants = &analysis.constants;
                self.function_values = &analysis.function_values;
                let constants = global.constant_values.iter().copied().collect();
                self.static_value(initializer, &constants)
            } else {
                if matches!(global.ty, Type::Struct(_) | Type::Array(_)) {
                    "{0}".to_owned()
                } else {
                    "0".to_owned()
                }
            };
            self.source_offset = Some(global.span.start);
            self.line(&format!(
                "static {}{}{} {} = {value};",
                c_type(global.ty),
                if global.readonly { " const" } else { "" },
                if global.volatile { " volatile" } else { "" },
                global.link_name
            ));
            self.source_offset = None;
        }
    }

    fn static_value(&self, expression: &Expression, constants: &HashMap<Span, i128>) -> String {
        let value = if let Some(name) = self.function_values.get(&expression.span) {
            format!(
                "(&{})",
                self.external_names
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| function_name(name))
            )
        } else if let Some(value) = constants.get(&expression.span) {
            self.integer_constant(expression, *value)
        } else {
            match &expression.kind {
                ExpressionKind::StringLiteral { bytes, .. } => {
                    let bytes = bytes
                        .iter()
                        .copied()
                        .chain(std::iter::once(0))
                        .map(|byte| byte.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{{ .crust_elements = {{ {bytes} }} }}")
                }
                ExpressionKind::ArrayLiteral { elements, .. } => format!(
                    "{{ .crust_elements = {{ {} }} }}",
                    elements
                        .iter()
                        .map(|element| self.static_value(element, constants))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                ExpressionKind::StructLiteral { fields, .. } => {
                    let Type::Struct(id) = self.expression_type(expression) else {
                        unreachable!()
                    };
                    let mut values = fields
                        .iter()
                        .map(|field| {
                            format!(
                                ".crust_field_{} = {}",
                                field.name,
                                self.static_value(&field.value, constants)
                            )
                        })
                        .collect::<Vec<_>>();
                    if self.types.structs[id].tagged {
                        let index = self.types.structs[id]
                            .fields
                            .iter()
                            .position(|field| field.name == fields[0].name)
                            .unwrap();
                        values.push(format!(".crust_tag = {index}"));
                    }
                    format!("{{ {} }}", values.join(", "))
                }
                ExpressionKind::Floating { spelling, .. } => spelling.clone(),
                ExpressionKind::Unary {
                    operator: UnaryOperator::Address,
                    operand,
                } => {
                    format!("(&({}))", self.static_place(operand, constants))
                }
                ExpressionKind::Unary { operator, operand } => {
                    let symbol = match operator {
                        UnaryOperator::Plus => "+",
                        UnaryOperator::Negate => "-",
                        UnaryOperator::Not => "!",
                        _ => unreachable!(),
                    };
                    format!("({symbol}({}))", self.static_value(operand, constants))
                }
                ExpressionKind::Binary {
                    operator,
                    left,
                    right,
                } => {
                    let symbol = match operator {
                        BinaryOperator::And => "&&",
                        BinaryOperator::Or => "||",
                        _ => operator.numeric_symbol(),
                    };
                    format!(
                        "({} {symbol} {})",
                        self.static_value(left, constants),
                        self.static_value(right, constants)
                    )
                }
                ExpressionKind::Conditional {
                    condition,
                    then_value,
                    else_value,
                } => format!(
                    "({} ? {} : {})",
                    self.static_value(condition, constants),
                    self.static_value(then_value, constants),
                    self.static_value(else_value, constants)
                ),
                ExpressionKind::Cast { operand, .. } => self.static_value(operand, constants),
                ExpressionKind::SizeofType(ty) => format!("sizeof({})", c_type(*ty)),
                ExpressionKind::Alignof(ty) => format!("_Alignof({})", c_type(*ty)),
                ExpressionKind::SizeofValue(value) => {
                    format!("sizeof({})", c_type(self.expression_type(value)))
                }
                _ => unreachable!("validated scalar static initializer"),
            }
        };
        if let Some(target) = self.conversions.get(&expression.span) {
            return format!("(({})({value}))", c_type(*target));
        }
        return value;
    }

    fn static_place(&self, expression: &Expression, constants: &HashMap<Span, i128>) -> String {
        return match &expression.kind {
            ExpressionKind::Variable { name, .. } => self.scopes[0].variables[name].name.clone(),
            ExpressionKind::Field { base, name, .. } => format!(
                "({}).crust_field_{name}",
                self.static_place(base, constants)
            ),
            ExpressionKind::Index { base, index } => format!(
                "({}).crust_elements[{}]",
                self.static_place(base, constants),
                self.static_value(index, constants)
            ),
            _ => unreachable!("checked static address"),
        };
    }
}
