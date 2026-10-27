use super::*;

const MAX_NESTING: usize = 64;

pub(super) fn ty(parser: &Parser, root: Type, span: Span) -> Result<(), Diagnostic> {
    let mut pending = vec![(root, 1)];
    let mut visited = HashMap::new();
    while let Some((ty, depth)) = pending.pop() {
        if depth > MAX_NESTING {
            return Err(Diagnostic::new(span, "type nesting limit exceeded (64)"));
        }
        if visited.get(&ty).is_some_and(|previous| *previous >= depth) {
            continue;
        }
        visited.insert(ty, depth);
        match ty {
            Type::Pointer(id) => pending.push((parser.pointers[id].pointee, depth + 1)),
            Type::Array(id) => pending.push((parser.arrays[id].element, depth + 1)),
            Type::Borrow { pointee, .. } => pending.push((pointee.ty(), depth + 1)),
            Type::Function(id) => {
                let signature = &parser.function_types[id];
                pending.push((signature.result, depth + 1));
                pending.extend(signature.parameters.iter().map(|ty| (*ty, depth + 1)));
            }
            Type::Integer(_)
            | Type::Bool
            | Type::Void
            | Type::BoxInt
            | Type::Struct(_)
            | Type::Enum(_) => {}
        }
    }
    return Ok(());
}

impl Parser {
    pub(super) fn nested<T>(
        &mut self,
        parse: impl FnOnce(&mut Self) -> Result<T, Diagnostic>,
    ) -> Result<T, Diagnostic> {
        if self.nesting >= MAX_NESTING {
            return Err(Diagnostic::new(
                self.current().span,
                "syntax nesting limit exceeded (64)",
            ));
        }
        self.nesting += 1;
        let result = parse(self);
        self.nesting -= 1;
        return result;
    }
}

// Left-associative operators and postfix chains build trees in a loop. Bound
// those trees before later recursive analysis, emission, or destruction.
pub(super) fn expression(root: &Expression) -> Result<(), Diagnostic> {
    let mut pending = vec![(root, 1)];
    while let Some((expression, depth)) = pending.pop() {
        if depth > MAX_NESTING {
            return Err(Diagnostic::new(
                expression.span,
                "expression nesting limit exceeded (64)",
            ));
        }
        let mut child = |expression| pending.push((expression, depth + 1));
        match &expression.kind {
            ExpressionKind::Storage { pointer, value } => {
                child(pointer);
                if let Some(value) = value {
                    child(value);
                }
            }
            ExpressionKind::IndirectCall { callee, arguments } => {
                child(callee);
                for argument in arguments {
                    child(argument);
                }
            }
            ExpressionKind::Index { base, index } => {
                child(base);
                child(index);
            }
            ExpressionKind::ArrayLiteral { elements, .. } => {
                for element in elements {
                    child(element);
                }
            }
            ExpressionKind::Unary { operand, .. }
            | ExpressionKind::Cast { operand, .. }
            | ExpressionKind::SizeofValue(operand) => child(operand),
            ExpressionKind::Call { arguments, .. }
            | ExpressionKind::StaticCall { arguments, .. } => {
                for argument in arguments {
                    child(argument);
                }
            }
            ExpressionKind::MethodCall {
                receiver,
                arguments,
                ..
            } => {
                child(receiver);
                for argument in arguments {
                    child(argument);
                }
            }
            ExpressionKind::Field { base, .. } => child(base),
            ExpressionKind::StructLiteral { fields, .. } => {
                for field in fields {
                    child(&field.value);
                }
            }
            ExpressionKind::Binary { left, right, .. } => {
                child(left);
                child(right);
            }
            ExpressionKind::StaticReference { .. }
            | ExpressionKind::EnumValue { .. }
            | ExpressionKind::StringLiteral { .. }
            | ExpressionKind::SizeofType(_)
            | ExpressionKind::Alignof(_)
            | ExpressionKind::Boolean(_)
            | ExpressionKind::Integer(_)
            | ExpressionKind::Variable { .. } => {}
        }
    }
    return Ok(());
}
