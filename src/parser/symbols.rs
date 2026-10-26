use super::*;

impl Parser {
    pub(super) fn reference_type(&mut self, span: Span, ty: Type) {
        let declaration = match ty {
            Type::Struct(id) => Some(TypeDeclaration::Aggregate(id)),
            Type::Enum(id) => Some(TypeDeclaration::Enum(id)),
            _ => None,
        };
        self.type_references
            .push(TypeReference { span, declaration });
    }

    pub(super) fn generic_parameter_declarations(
        &mut self,
        start: usize,
        end: usize,
        container: &str,
    ) -> Vec<Span> {
        let mut spans = Vec::new();
        let mut parameter: Option<Span> = None;
        for position in start..end {
            let token = &self.tokens[position];
            if let Token::Identifier(name) = &token.kind
                && matches!(self.tokens[position - 1].kind, Token::Colon | Token::Plus)
            {
                self.generics
                    .constraint_references
                    .push((name.clone(), token.span));
            }
            if parameter.is_none() && matches!(token.kind, Token::Identifier(_)) {
                parameter = Some(token.span);
                spans.push(token.span);
            }
            if matches!(token.kind, Token::Comma | Token::Greater)
                && let Some(name_span) = parameter.take()
            {
                let header_span = name_span.through(self.tokens[position - 1].span);
                self.source_declarations.push(SourceDeclaration {
                    kind: SourceDeclarationKind::TypeParameter,
                    name_span,
                    header_span,
                    span: header_span,
                    container: Some(container.to_owned()),
                });
            }
        }
        return spans;
    }
}
