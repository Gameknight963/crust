use super::*;

impl FunctionChecker<'_> {
    pub(super) fn record_retention(
        &mut self,
        place: &Place,
        captures: &captures::Captures,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let captured_destination = place
            .root
            .1
            .strip_prefix("!capture:")
            .and_then(|name| name.split_once(':'))
            .map(|(name, _)| name);
        if self.is_destructor && captured_destination.is_none() {
            return Ok(());
        }
        let Some(destination) = self.parameters.iter().position(|parameter| {
            place.root.0 == 1
                && (captured_destination == Some(parameter.name.as_str())
                    || (place.root.1 == parameter.name
                        && matches!(parameter.ty, Type::Borrow { mutable: true, .. })))
        }) else {
            return Ok(());
        };
        let mut roots = Vec::new();
        for capture in captures {
            let loan = self
                .loans
                .iter()
                .find(|loan| loan.id == capture.id())
                .expect("retained source loan");
            roots.push(loan.root.clone());
            roots.extend(loan.alternatives.iter().map(|(root, _)| root.clone()));
        }
        for root in roots {
            if root.0 == 0 {
                continue;
            }
            let captured = root
                .1
                .strip_prefix("!capture:")
                .and_then(|name| name.split_once(':'))
                .map(|(name, _)| name);
            let source = if root.0 == 1 {
                self.parameters.iter().position(|parameter| {
                    captured == Some(parameter.name.as_str())
                        || (root.1 == parameter.name && parameter.ty.is_borrow())
                })
            } else {
                None
            };
            let Some(source) = source else {
                return Err(Diagnostic::new(
                    span,
                    "a stored reference cannot outlive a local or a value parameter's own storage",
                ));
            };
            if self.is_destructor {
                return Err(Diagnostic::new(
                    span,
                    "destructors cannot retain references in external storage",
                ));
            }
            self.stored_borrows
                .insert((destination, place.ty, source, captured.is_some()));
        }
        if self.stored_borrows.len() > 4096 {
            return Err(Diagnostic::new(
                span,
                "stored-reference inference exceeds 4096 relationships",
            ));
        }
        return Ok(());
    }

    pub(super) fn apply_retention(
        &mut self,
        signature: &Signature,
        arguments: &[Expression],
        previous_loans: usize,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let mut effects: Vec<_> = signature.stored_borrows.iter().copied().collect();
        effects.sort_by_key(|(destination, ty, source, captured)| {
            (*destination, format!("{ty:?}"), *source, *captured)
        });
        for (destination, target_type, source, captured_only) in effects {
            let sources = if captured_only {
                self.stored_argument_sources(
                    &arguments[source],
                    signature.parameters[source],
                    span,
                )?
            } else {
                let mut values = self
                    .capture_expressions
                    .get(&arguments[source].span)
                    .cloned()
                    .unwrap_or_default();
                if let Some(id) = self.references.get(&arguments[source].span) {
                    values.push(captures::Capture::new(*id));
                }
                values
            };
            if sources.is_empty() {
                continue;
            }
            let mut destination_ids: Vec<_> = self
                .capture_expressions
                .get(&arguments[destination].span)
                .into_iter()
                .flatten()
                .map(captures::Capture::id)
                .collect();
            if let Some(id) = self.references.get(&arguments[destination].span) {
                destination_ids.push(*id);
            }
            let mut roots = Vec::new();
            for id in destination_ids {
                let loan = self
                    .loans
                    .iter()
                    .find(|loan| loan.id == id)
                    .expect("retaining destination loan");
                roots.push((loan.root.clone(), loan.fields.clone()));
                roots.extend(loan.alternatives.clone());
            }
            roots.sort();
            roots.dedup();
            let mut fields = Vec::new();
            captures::reference_fields(self.types, target_type, Vec::new(), &mut fields, span)?;
            let source_ids: Vec<_> = sources.iter().map(captures::Capture::id).collect();
            let mut shared = None;
            let mut exclusive = None;
            for (root, projection) in roots {
                let mut candidates = self
                    .borrowed_storage_types
                    .get(&root)
                    .cloned()
                    .unwrap_or_default();
                if let Some(variable) = self.scopes.get(root.0).and_then(|scope| scope.get(&root.1))
                {
                    candidates.insert(variable.ty.referent());
                }
                for ty in candidates {
                    let mut regions = Vec::new();
                    retention_regions(
                        self.types,
                        ty,
                        &projection,
                        Vec::new(),
                        target_type,
                        &mut regions,
                        span,
                    )?;
                    for projection in regions {
                        let place = Place {
                            root: root.clone(),
                            fields: projection,
                            ty: target_type,
                            mutable: true,
                            volatile: false,
                            via: None,
                        };
                        let mut captured = Vec::new();
                        for (path, ty) in &fields {
                            let mutable = matches!(ty, Type::Borrow { mutable: true, .. });
                            let slot = if mutable { &mut exclusive } else { &mut shared };
                            let id = if let Some(id) = *slot {
                                id
                            } else {
                                let id = self.retained_source(&source_ids, mutable, previous_loans);
                                *slot = Some(id);
                                id
                            };
                            captured.push(captures::Capture::at(path.clone(), id));
                        }
                        let retained: Vec<_> = captured.iter().map(captures::Capture::id).collect();
                        self.bind_capture_set(&place, captured, span)?;
                        if signature
                            .return_borrow
                            .as_ref()
                            .is_some_and(|sources| sources.contains(&source))
                        {
                            let values = self
                                .capture_expressions
                                .entry(arguments[source].span)
                                .or_default();
                            for id in retained {
                                let capture = captures::Capture::new(id);
                                if !values.contains(&capture) {
                                    values.push(capture);
                                }
                            }
                        }
                    }
                }
            }
        }
        return Ok(());
    }
    fn stored_argument_sources(
        &self,
        argument: &Expression,
        parameter: Type,
        span: Span,
    ) -> Result<captures::Captures, Diagnostic> {
        if !parameter.is_borrow() {
            return Ok(self
                .capture_expressions
                .get(&argument.span)
                .cloned()
                .unwrap_or_default());
        }
        let Some(id) = self.references.get(&argument.span) else {
            return Ok(Vec::new());
        };
        let loan = self
            .loans
            .iter()
            .find(|loan| loan.id == *id)
            .expect("stored argument reference");
        let mut origins = vec![(loan.root.clone(), loan.fields.clone())];
        origins.extend(loan.alternatives.clone());
        let mut output = Vec::new();
        for (root, projection) in origins {
            let mut candidates = self
                .borrowed_storage_types
                .get(&root)
                .cloned()
                .unwrap_or_default();
            if let Some(variable) = self.scopes.get(root.0).and_then(|scope| scope.get(&root.1)) {
                candidates.insert(variable.ty.referent());
            }
            for ty in candidates {
                let mut regions = Vec::new();
                retention_regions(
                    self.types,
                    ty,
                    &projection,
                    Vec::new(),
                    parameter.referent(),
                    &mut regions,
                    span,
                )?;
                for fields in regions {
                    let place = Place {
                        root: root.clone(),
                        fields,
                        ty: parameter.referent(),
                        mutable: false,
                        volatile: false,
                        via: None,
                    };
                    output.extend(self.captures_at(&place));
                }
            }
        }
        return Ok(output);
    }

    fn retained_source(&mut self, sources: &[usize], mutable: bool, previous: usize) -> usize {
        let mut origins = Vec::new();
        let mut parents = Vec::new();
        for id in sources {
            let loan = self
                .loans
                .iter()
                .find(|loan| loan.id == *id)
                .expect("stored argument source");
            origins.push((loan.root.clone(), loan.fields.clone()));
            origins.extend(loan.alternatives.clone());
            let mut pending = vec![*id];
            let mut seen = HashSet::new();
            while let Some(id) = pending.pop() {
                if !seen.insert(id) {
                    continue;
                }
                let index = self
                    .loans
                    .iter()
                    .position(|loan| loan.id == id)
                    .expect("stored source ancestor");
                if index < previous || self.loans[index].scope.is_some() {
                    parents.push(id);
                } else {
                    pending.extend(&self.loans[index].parents);
                }
            }
        }
        origins.sort();
        origins.dedup();
        parents.sort_unstable();
        parents.dedup();
        let (root, fields) = origins.remove(0);
        let id = self.next_loan;
        self.next_loan += 1;
        self.loans.push(Loan {
            id,
            root,
            fields,
            alternatives: origins,
            parents,
            mutable,
            scope: None,
        });
        return id;
    }
}

fn retention_regions(
    types: &Types,
    ty: Type,
    projection: &[String],
    path: Vec<String>,
    target: Type,
    output: &mut Vec<Vec<String>>,
    span: Span,
) -> Result<(), Diagnostic> {
    return retention_regions_inner(
        types,
        ty,
        projection,
        path,
        target,
        output,
        span,
        &mut HashSet::new(),
    );
}

#[allow(clippy::too_many_arguments)]
fn retention_regions_inner(
    types: &Types,
    ty: Type,
    projection: &[String],
    path: Vec<String>,
    target: Type,
    output: &mut Vec<Vec<String>>,
    span: Span,
    active: &mut HashSet<(Type, usize)>,
) -> Result<(), Diagnostic> {
    if !active.insert((ty, projection.len())) {
        return Ok(());
    }
    if !types.contains_borrows(ty) {
        return Ok(());
    }
    if output.len() >= 4096 {
        return Err(Diagnostic::new(
            span,
            "stored-reference inference exceeds 4096 regions",
        ));
    }
    if projection.is_empty() && ty == target {
        output.push(path);
        active.remove(&(ty, projection.len()));
        return Ok(());
    }
    match ty {
        Type::Pointer(id) => retention_regions_inner(
            types,
            types.pointers[id].pointee,
            projection,
            path,
            target,
            output,
            span,
            active,
        )?,
        Type::Array(id) => retention_regions_inner(
            types,
            types.arrays[id].element,
            projection,
            path,
            target,
            output,
            span,
            active,
        )?,
        Type::Struct(id) => {
            for field in &types.structs[id].fields {
                let name = if types.structs[id].tagged {
                    "*"
                } else {
                    &field.name
                };
                if projection
                    .first()
                    .is_some_and(|requested| requested != name)
                {
                    continue;
                }
                let mut nested = path.clone();
                nested.push(name.to_owned());
                let remaining = if projection.is_empty() {
                    projection
                } else {
                    &projection[1..]
                };
                retention_regions(types, field.ty, remaining, nested, target, output, span)?;
            }
        }
        _ => {}
    }
    active.remove(&(ty, projection.len()));
    return Ok(());
}

#[derive(Clone)]
pub(super) struct BorrowState {
    loans: Vec<Loan>,
    captures: HashMap<captures::Root, captures::Captures>,
}

impl FunctionChecker<'_> {
    pub(super) fn save_borrows(&self) -> BorrowState {
        return BorrowState {
            loans: self.loans.clone(),
            captures: self.capture_bindings.clone(),
        };
    }
    pub(super) fn restore_borrows(&mut self, state: &BorrowState) {
        self.loans = state.loans.clone();
        self.capture_bindings = state.captures.clone();
    }
    pub(super) fn merge_borrows(&mut self, states: &[BorrowState]) {
        let mut loans: HashMap<usize, Loan> = HashMap::new();
        let mut captures: HashMap<captures::Root, captures::Captures> = HashMap::new();
        for state in states {
            for loan in &state.loans {
                if let Some(previous) = loans.get_mut(&loan.id) {
                    previous.scope = match (previous.scope, loan.scope) {
                        (Some(a), Some(b)) => Some(a.min(b)),
                        (a, b) => a.or(b),
                    };
                } else {
                    loans.insert(loan.id, loan.clone());
                }
            }
            for (root, values) in &state.captures {
                let target = captures.entry(root.clone()).or_default();
                for capture in values {
                    if !target.contains(capture) {
                        target.push(capture.clone());
                    }
                }
            }
        }
        self.loans = loans.into_values().collect();
        self.loans.sort_by_key(|loan| loan.id);
        self.capture_bindings = captures;
    }
}

impl FunctionChecker<'_> {
    pub(super) fn copy_captures(&mut self, target: Span, source: Span) {
        let captures = self
            .capture_expressions
            .get(&source)
            .cloned()
            .unwrap_or_default();
        self.remember_captures(target, captures);
    }
    pub(super) fn raw_storage_target(
        &mut self,
        expression: &Expression,
    ) -> Result<Option<Place>, Diagnostic> {
        return match &expression.kind {
            ExpressionKind::Binary { left, right, .. } => {
                if matches!(
                    self.expression_types.get(&left.span),
                    Some(Type::Pointer(_))
                ) {
                    self.raw_storage_target(left)
                } else {
                    self.raw_storage_target(right)
                }
            }
            ExpressionKind::Cast { operand, .. } => self.raw_storage_target(operand),
            ExpressionKind::Unary {
                operator: UnaryOperator::Address,
                operand,
            } => self.checked_place(operand).map(Some),
            ExpressionKind::Variable { .. }
            | ExpressionKind::Field { .. }
            | ExpressionKind::Index { .. } => self.checked_place(expression).map(Some),
            _ => Ok(None),
        };
    }
}
