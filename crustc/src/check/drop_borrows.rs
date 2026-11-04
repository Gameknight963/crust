use super::*;

impl FunctionChecker<'_> {
    pub(super) fn check_source_drop_order(
        &self,
        destination: &Place,
        source: &captures::Root,
        span: Span,
    ) -> Result<(), Diagnostic> {
        if source.0 == destination.root.0
            && let Some(scope) = self.scopes.get(source.0)
            && let (Some(holder), Some(origin)) =
                (scope.get(&destination.root.1), scope.get(&source.1))
            && self.types.owns(origin.ty)
            && holder
                .declaration
                .zip(origin.declaration)
                .is_some_and(|(holder, origin)| holder.start < origin.start)
        {
            return Err(Diagnostic::new(
                span,
                "reference field source would be destroyed before its holder",
            ));
        }
        return Ok(());
    }

    pub(super) fn check_scope_drops(
        &self,
        first_scope: usize,
        span: Span,
    ) -> Result<(), Diagnostic> {
        return self.check_drops(first_scope, span, &[]);
    }

    fn check_drops(
        &self,
        first_scope: usize,
        span: Span,
        temporaries: &[usize],
    ) -> Result<(), Diagnostic> {
        let mut ending: Vec<_> = self
            .loans
            .iter()
            .filter(|loan| loan.scope.is_some_and(|scope| scope >= first_scope))
            .map(|loan| loan.id)
            .collect();
        ending.extend_from_slice(temporaries);
        for (index, scope) in self.scopes.iter().enumerate().skip(first_scope) {
            for (name, variable) in scope {
                if !self.types.owns(variable.ty)
                    || matches!(variable.state, State::Uninitialized | State::Moved)
                {
                    continue;
                }
                let root = (index, name.clone());
                self.access_through(&root, &[], true, &ending, span)?;
                if !self.types.destructor_uses_borrows(variable.ty) {
                    continue;
                }
                let mut parents = ending.clone();
                let captures = self
                    .capture_bindings
                    .get(&root)
                    .cloned()
                    .unwrap_or_default();
                parents.extend(captures.iter().map(|capture| capture.id()));
                for capture in captures {
                    let loan = self
                        .loans
                        .iter()
                        .find(|loan| loan.id == capture.id())
                        .expect("drop capture is live");
                    for (root, fields) in std::iter::once((&loan.root, &loan.fields)).chain(
                        loan.alternatives
                            .iter()
                            .map(|(root, fields)| (root, fields)),
                    ) {
                        self.access_through(root, fields, loan.mutable, &parents, span)?;
                    }
                }
            }
        }
        return Ok(());
    }

    pub(super) fn check_return_drops(
        &mut self,
        value: Option<&Expression>,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let mut escaping = HashSet::new();
        if self.return_type.is_borrow()
            && let Some(value) = value
            && let Some(id) = self.references.get(&value.span)
        {
            escaping.insert(*id);
        }
        if self.types.carries_borrows(self.return_type)
            && let Some(value) = value
            && let Some(captures) = self.capture_expressions.get(&value.span)
            && !captures.is_empty()
        {
            let parents: Vec<_> = captures.iter().map(|capture| capture.id()).collect();
            let mutable = self.types.contains_mutable_borrows(self.return_type);
            // This loan survives the ending locals while their destructors
            // run. The value's intrinsic loans can themselves end here.
            escaping.insert(self.capture_loan(&parents, mutable));
        }
        let temporaries: Vec<_> = self
            .loans
            .iter()
            .filter(|loan| loan.scope.is_none() && !escaping.contains(&loan.id))
            .map(|loan| loan.id)
            .collect();
        return self.check_drops(1, span, &temporaries);
    }
}
