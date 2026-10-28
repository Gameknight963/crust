use super::*;

pub(super) fn has_private_fields(ty: Type, types: &Types) -> bool {
    return match ty {
        Type::Struct(id) => types.structs[id].fields.iter().any(|field| {
            field.private || field.ty.is_borrow() || has_private_fields(field.ty, types)
        }),
        Type::Array(id) => has_private_fields(types.arrays[id].element, types),
        _ => false,
    };
}

pub(super) fn validate(
    global: &ExternalVariable,
    analysis: &FunctionAnalysis,
    types: &Types,
) -> Result<(), Diagnostic> {
    let constants: HashMap<_, _> = global.constant_values.iter().copied().collect();
    fn address(
        expression: &Expression,
        analysis: &FunctionAnalysis,
        constants: &HashMap<Span, i128>,
        types: &Types,
    ) -> Result<(), Diagnostic> {
        match &expression.kind {
            ExpressionKind::Variable { .. } => {}
            ExpressionKind::Field { base, .. } => address(base, analysis, constants, types)?,
            ExpressionKind::Index { base, index } => {
                address(base, analysis, constants, types)?;
                let ty = analysis
                    .receiver_types
                    .get(&base.span)
                    .or_else(|| analysis.expression_types.get(&base.span))
                    .copied()
                    .unwrap()
                    .referent();
                let Type::Array(id) = ty else {
                    return Err(Diagnostic::new(
                        base.span,
                        "static indexing requires an array object",
                    ));
                };
                let value = constants
                    .get(&index.span)
                    .or_else(|| analysis.constants.get(&index.span))
                    .copied()
                    .ok_or_else(|| {
                        Diagnostic::new(index.span, "static indexing requires an integer constant")
                    })?;
                if value < 0 || value >= types.arrays[id].length as i128 {
                    return Err(Diagnostic::new(
                        index.span,
                        "static array index is out of bounds",
                    ));
                }
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
    fn visit(
        expression: &Expression,
        analysis: &FunctionAnalysis,
        constants: &HashMap<Span, i128>,
        types: &Types,
    ) -> Result<(), Diagnostic> {
        if constants.contains_key(&expression.span)
            || analysis.function_values.contains_key(&expression.span)
        {
            return Ok(());
        }
        match &expression.kind {
            ExpressionKind::StringLiteral { .. } => {}
            ExpressionKind::ArrayLiteral { elements, .. } => {
                for element in elements {
                    visit(element, analysis, constants, types)?;
                }
            }
            ExpressionKind::StructLiteral { fields, .. } => {
                for field in fields {
                    visit(&field.value, analysis, constants, types)?;
                }
            }
            ExpressionKind::Floating { .. }
            | ExpressionKind::SizeofType(_)
            | ExpressionKind::SizeofValue(_)
            | ExpressionKind::Alignof(_) => {}
            ExpressionKind::Unary {
                operator: UnaryOperator::Address,
                operand,
            } => {
                address(operand, analysis, constants, types)?;
            }
            ExpressionKind::Unary { operand, .. } | ExpressionKind::Cast { operand, .. } => {
                visit(operand, analysis, constants, types)?
            }
            ExpressionKind::Binary { left, right, .. } => {
                visit(left, analysis, constants, types)?;
                visit(right, analysis, constants, types)?;
            }
            ExpressionKind::Conditional {
                condition,
                then_value,
                else_value,
            } => {
                visit(condition, analysis, constants, types)?;
                visit(then_value, analysis, constants, types)?;
                visit(else_value, analysis, constants, types)?;
            }
            _ => {
                return Err(Diagnostic::new(
                    expression.span,
                    "static storage requires a constant value or address",
                ));
            }
        }
        return Ok(());
    }
    visit(
        global.initializer.as_ref().unwrap(),
        analysis,
        &constants,
        types,
    )?;
    for (span, target) in &analysis.conversions {
        if constants
            .keys()
            .any(|parent| parent != span && parent.start <= span.start && parent.end >= span.end)
        {
            continue;
        }
        let source = analysis.expression_types[span];
        if *target == Type::Bool
            || matches!(source, Type::Pointer(_))
            || matches!(target, Type::Pointer(_))
        {
            continue;
        }
        if let Some(target) = types.integer_representation(*target) {
            if let Some(value) = constants.get(span) {
                if !target.fits(*value) {
                    return Err(Diagnostic::new(
                        *span,
                        "static integer conversion is out of range",
                    ));
                }
            } else if let Some(source) = types.integer_representation(source) {
                if source.minimum() < target.minimum() || source.maximum() > target.maximum() {
                    return Err(Diagnostic::new(
                        *span,
                        "static narrowing requires an evaluated integer constant",
                    ));
                }
            } else {
                return Err(Diagnostic::new(
                    *span,
                    "floating-to-integer static initialization requires an integer constant",
                ));
            }
        }
        if let (Type::Float(source), Type::Float(target)) = (source, *target)
            && source > target
        {
            return Err(Diagnostic::new(
                *span,
                "static floating initialization requires a literal of the destination type",
            ));
        }
    }
    return Ok(());
}
