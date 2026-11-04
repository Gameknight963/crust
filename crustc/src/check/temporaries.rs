use super::*;

impl FunctionChecker<'_> {
    pub(super) fn end_loans_since(&mut self, count: usize) {
        let retained: Vec<_> = self
            .loans
            .drain(count..)
            .filter(|loan| loan.scope.is_some())
            .collect();
        self.loans.extend(retained);
    }

    pub(super) fn check_named_reference(&self, id: usize, span: Span) -> Result<(), Diagnostic> {
        if let Some(loan) = self.loans.iter().find(|loan| loan.id == id)
            && (loan.root.1.starts_with("!temporary:")
                || loan
                    .alternatives
                    .iter()
                    .any(|(root, _)| root.1.starts_with("!temporary:")))
        {
            return Err(Diagnostic::new(
                span,
                "a stored reference requires a named owner",
            ));
        }
        return Ok(());
    }

    pub(super) fn owner_temporary(
        &mut self,
        expression: &Expression,
        ty: Type,
    ) -> Result<Place, Diagnostic> {
        let root = (
            self.scopes.len(),
            format!(
                "!temporary:{}:{}",
                expression.span.start, expression.span.end
            ),
        );
        if !self.unevaluated {
            self.borrowed_storage_types
                .entry(root.clone())
                .or_default()
                .insert(ty);
            self.temporary_owners.insert(expression.span, ty);
            let captures = self
                .capture_expressions
                .get(&expression.span)
                .cloned()
                .unwrap_or_default();
            let mut pending: Vec<_> = captures.iter().map(captures::Capture::id).collect();
            let mut seen = HashSet::new();
            while let Some(id) = pending.pop() {
                if !seen.insert(id) {
                    continue;
                }
                if let Some(loan) = self.loans.iter_mut().find(|loan| loan.id == id) {
                    if loan.scope.is_none() {
                        loan.scope = Some(usize::MAX);
                    }
                    pending.extend(&loan.parents);
                }
            }
            self.capture_bindings.insert(root.clone(), captures);
        }
        self.receiver_types.insert(expression.span, ty);
        return Ok(Place {
            ty,
            root,
            fields: Vec::new(),
            mutable: true,
            volatile: false,
            via: None,
        });
    }
}
