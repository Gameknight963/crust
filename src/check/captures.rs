use super::*;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Capture {
    path: Vec<String>,
    loan: usize,
}

impl Capture {
    pub(super) fn new(loan: usize) -> Self {
        return Self {
            path: Vec::new(),
            loan,
        };
    }
    pub(super) fn at(path: Vec<String>, loan: usize) -> Self {
        return Self { path, loan };
    }
    pub(super) fn id(&self) -> usize {
        return self.loan;
    }
    pub(super) fn prepend(&mut self, field: &str) {
        self.path.insert(0, field.to_owned());
    }
}

pub(super) type Captures = Vec<Capture>;
pub(super) type Root = (usize, String);

pub(super) fn reference_fields(
    types: &Types,
    ty: Type,
    path: Vec<String>,
    output: &mut Vec<(Vec<String>, Type)>,
    span: Span,
) -> Result<(), Diagnostic> {
    if !types.contains_borrows(ty) {
        return Ok(());
    }
    if output.len() >= 4096 {
        return Err(Diagnostic::new(
            span,
            "reference-field tracking exceeds 4096 fields",
        ));
    }
    match ty {
        Type::Borrow { .. } => output.push((path, ty)),
        Type::Struct(id) => {
            for field in &types.structs[id].fields {
                let mut nested = path.clone();
                nested.push(
                    if types.structs[id].tagged {
                        "*"
                    } else {
                        &field.name
                    }
                    .to_owned(),
                );
                reference_fields(types, field.ty, nested, output, field.span)?;
            }
        }
        Type::Array(id) => {
            reference_fields(
                types,
                types.arrays[id].element,
                path,
                output,
                types.arrays[id].span,
            )?;
        }
        _ => {}
    }
    return Ok(());
}

impl FunctionChecker<'_> {
    pub(super) fn initialize_captures(&mut self, parameter: &Parameter) -> Result<(), Diagnostic> {
        let mut fields = Vec::new();
        reference_fields(
            self.types,
            parameter.ty.referent(),
            Vec::new(),
            &mut fields,
            parameter.span,
        )?;
        let mut captures = Vec::new();
        for (path, ty) in fields {
            let root = (1, format!("!capture:{}:{}", parameter.name, path.join(".")));
            let id = self.next_loan;
            self.next_loan += 1;
            self.loans.push(Loan {
                id,
                root: root.clone(),
                fields: Vec::new(),
                mutable: matches!(ty, Type::Borrow { mutable: true, .. }),
                parents: Vec::new(),
                alternatives: Vec::new(),
                scope: Some(1),
            });
            if self
                .return_borrow_root
                .as_ref()
                .is_some_and(|roots| roots.contains(&(1, parameter.name.clone())))
            {
                self.capture_return_roots.insert(root);
            }
            captures.push(Capture { path, loan: id });
        }
        if !captures.is_empty() {
            self.capture_bindings
                .insert((1, parameter.name.clone()), captures);
        }
        return Ok(());
    }

    pub(super) fn captures_at(&self, place: &Place) -> Captures {
        return self
            .capture_bindings
            .get(&place.root)
            .into_iter()
            .flatten()
            .filter_map(|capture| {
                capture
                    .path
                    .strip_prefix(place.fields.as_slice())
                    .map(|path| Capture {
                        path: path.to_vec(),
                        loan: capture.loan,
                    })
            })
            .collect();
    }

    pub(super) fn remember_captures(&mut self, span: Span, captures: Captures) {
        if !captures.is_empty() {
            self.capture_expressions.insert(span, captures);
        }
    }

    pub(super) fn capture_value(&mut self, span: Span, place: &Place) {
        if !self.types.contains_borrows(place.ty) {
            return;
        }
        let mut captures = self.captures_at(place);
        if let Some(parent) = place.via {
            for capture in &mut captures {
                capture.loan = self.capture_loan(&[parent, capture.loan], false);
            }
        }
        self.remember_captures(span, captures);
    }

    pub(super) fn bind_captures(
        &mut self,
        place: &Place,
        value: &Expression,
    ) -> Result<(), Diagnostic> {
        let captures = self
            .capture_expressions
            .get(&value.span)
            .cloned()
            .unwrap_or_default();
        if captures.len() > 4096 {
            return Err(Diagnostic::new(
                value.span,
                "reference-field tracking exceeds 4096 fields",
            ));
        }
        for capture in &captures {
            let loan = self
                .loans
                .iter()
                .find(|loan| loan.id == capture.loan)
                .expect("captured loan is live");
            for (root, _) in std::iter::once((&loan.root, &loan.fields)).chain(
                loan.alternatives
                    .iter()
                    .map(|(root, fields)| (root, fields)),
            ) {
                self.check_source_drop_order(place, root, value.span)?;
                if root.0 > place.root.0 {
                    return Err(Diagnostic::new(
                        value.span,
                        "reference field would outlive its source",
                    ));
                }
            }
        }
        for capture in &captures {
            let mut pending = vec![capture.loan];
            let mut seen = HashSet::new();
            while let Some(id) = pending.pop() {
                if !seen.insert(id) {
                    continue;
                }
                let loan = self
                    .loans
                    .iter_mut()
                    .find(|loan| loan.id == id)
                    .expect("captured ancestry is live");
                loan.scope = Some(
                    loan.scope
                        .map_or(place.root.0, |scope| scope.min(place.root.0)),
                );
                pending.extend(&loan.parents);
            }
        }
        let destination = self.capture_bindings.entry(place.root.clone()).or_default();
        for mut capture in captures {
            let mut path = place.fields.clone();
            path.extend(capture.path);
            capture.path = path;
            if !destination.contains(&capture) {
                if destination.len() >= 4096 {
                    return Err(Diagnostic::new(
                        value.span,
                        "reference-field tracking exceeds 4096 fields",
                    ));
                }
                destination.push(capture);
            }
        }
        return Ok(());
    }

    pub(super) fn check_capture_move(&self, place: &Place, span: Span) -> Result<(), Diagnostic> {
        if !self.types.owns(place.ty) {
            return Ok(());
        }
        let captures = self.captures_at(place);
        let parents: Vec<_> = captures.iter().map(|capture| capture.loan).collect();
        for capture in captures {
            let loan = self
                .loans
                .iter()
                .find(|loan| loan.id == capture.loan)
                .expect("captured loan is live");
            for (root, fields) in std::iter::once((&loan.root, &loan.fields)).chain(
                loan.alternatives
                    .iter()
                    .map(|(root, fields)| (root, fields)),
            ) {
                self.access_through(root, fields, loan.mutable, &parents, span)?;
            }
        }
        return Ok(());
    }

    pub(super) fn stored_reference(
        &mut self,
        expression: &Expression,
        place: &Place,
    ) -> Result<Type, Diagnostic> {
        let Type::Borrow {
            pointee,
            mutable,
            volatile,
        } = place.ty
        else {
            unreachable!()
        };
        self.access(
            &place.root,
            &place.fields,
            false,
            place.via,
            expression.span,
        )?;
        let captures = self.captures_at(place);
        let mut parents: Vec<_> = captures.iter().map(|capture| capture.loan).collect();
        if let Some(parent) = place.via
            && !parents.is_empty()
            && !parents.contains(&parent)
        {
            parents.insert(0, parent);
        }
        let id = if parents.len() == 1 {
            parents[0]
        } else if !parents.is_empty() {
            self.capture_loan(&parents, false)
        } else {
            // References reached through a borrowed object are valid for at
            // least that object's loan, even when their ultimate source is
            // hidden behind another reference field.
            self.borrow(
                place.root.clone(),
                place.fields.clone(),
                false,
                place.via,
                expression.span,
            )?
        };
        self.references.insert(expression.span, id);
        return Ok(Type::Borrow {
            pointee,
            mutable: mutable && place.mutable,
            volatile,
        });
    }

    pub(super) fn capture_loan(&mut self, parents: &[usize], mutable: bool) -> usize {
        let mut origins = Vec::new();
        for id in parents {
            let loan = self
                .loans
                .iter()
                .find(|loan| loan.id == *id)
                .expect("capture parent is live");
            origins.push((loan.root.clone(), loan.fields.clone()));
            origins.extend(loan.alternatives.clone());
        }
        let mut seen = HashSet::new();
        origins.retain(|origin| seen.insert(origin.clone()));
        let (root, fields) = origins.remove(0);
        let id = self.next_loan;
        self.next_loan += 1;
        self.loans.push(Loan {
            id,
            root,
            fields,
            alternatives: origins,
            parents: parents.to_vec(),
            mutable,
            scope: None,
        });
        return id;
    }

    pub(super) fn check_capture_return(&self, expression: &Expression) -> Result<(), Diagnostic> {
        for capture in self
            .capture_expressions
            .get(&expression.span)
            .into_iter()
            .flatten()
        {
            let loan = self
                .loans
                .iter()
                .find(|loan| loan.id == capture.loan)
                .expect("returned capture is live");
            for (root, _) in std::iter::once((&loan.root, &loan.fields)).chain(
                loan.alternatives
                    .iter()
                    .map(|(root, fields)| (root, fields)),
            ) {
                if !self.capture_return_roots.contains(root) {
                    return Err(Diagnostic::new(
                        expression.span,
                        "returned reference fields must originate from the declared borrow sources",
                    ));
                }
            }
        }
        return Ok(());
    }
}
