use super::*;

#[derive(Debug)]
pub(super) struct Scope {
    range: Location,
    declarations: Vec<(Location, usize)>,
}

impl Analysis {
    pub fn completions_at(&self, file: usize, offset: usize) -> Vec<&Symbol> {
        if !self
            .files
            .get(file)
            .is_some_and(|file| file.text.is_char_boundary(offset))
        {
            return Vec::new();
        }
        let mut visible = HashMap::new();
        for scope in &self.completion_scopes {
            if !contains(scope.range, file, offset, false) {
                continue;
            }
            let size = scope.range.span.end - scope.range.span.start;
            for (declaration, start) in &scope.declarations {
                if *start <= offset {
                    visible
                        .entry(*declaration)
                        .and_modify(|priority: &mut usize| *priority = (*priority).min(size))
                        .or_insert(size);
                }
            }
        }
        let mut candidates: Vec<_> = self
            .symbols
            .iter()
            .filter_map(|symbol| {
                if self.scoped_enum_members.contains(&symbol.location) {
                    return None;
                }
                let file_local = symbol.kind == SymbolKind::Function
                    && symbol.detail.split_whitespace().next() == Some("static");
                if file_local && symbol.location.file != file {
                    return None;
                }
                let priority = visible.get(&symbol.location).copied().or_else(|| {
                    return matches!(
                        symbol.kind,
                        SymbolKind::Function
                            | SymbolKind::Struct
                            | SymbolKind::Union
                            | SymbolKind::Enum
                            | SymbolKind::EnumMember
                            | SymbolKind::Interface
                            | SymbolKind::Alias
                            | SymbolKind::Global
                    )
                    .then_some(if file_local {
                        usize::MAX - 1
                    } else {
                        usize::MAX
                    });
                })?;
                return Some((priority, symbol));
            })
            .collect();
        candidates.sort_by_key(|(priority, symbol)| {
            return (*priority, std::cmp::Reverse(symbol.location.span.start));
        });
        let mut names = HashSet::new();
        let mut symbols: Vec<_> = candidates
            .into_iter()
            .filter_map(|(_, symbol)| names.insert(&symbol.name).then_some(symbol))
            .collect();
        symbols.sort_by(|a, b| a.name.cmp(&b.name));
        return symbols;
    }
}

fn block_scopes(block: &Block, locate: &impl Fn(Span) -> Location, scopes: &mut Vec<Scope>) {
    let mut declarations = Vec::new();
    for statement in &block.statements {
        match &statement.kind {
            StatementKind::Local { name_span, .. } => {
                declarations.push((*name_span, name_span.end));
            }
            StatementKind::Block(block) | StatementKind::Unsafe(block) => {
                block_scopes(block, locate, scopes);
            }
            StatementKind::If {
                then_block,
                else_block,
                ..
            } => {
                block_scopes(then_block, locate, scopes);
                if let Some(block) = else_block {
                    block_scopes(block, locate, scopes);
                }
            }
            StatementKind::While { body, .. } | StatementKind::DoWhile { body, .. } => {
                block_scopes(body, locate, scopes)
            }
            _ => {}
        }
    }
    let declarations = declarations
        .into_iter()
        .map(|(span, end)| {
            let location = locate(span);
            let end = locate(Span {
                start: end,
                end,
                ..span
            })
            .span
            .start;
            return (location, end);
        })
        .collect();
    scopes.push(Scope {
        range: locate(block.span),
        declarations,
    });
}

pub(super) fn scopes(checked: &AnalyzedProgram, locate: &impl Fn(Span) -> Location) -> Vec<Scope> {
    let mut scopes = Vec::new();
    for function in &checked.program.functions {
        let range = locate(function.body.span);
        scopes.push(Scope {
            range,
            declarations: function
                .parameters
                .iter()
                .filter(|parameter| !parameter.synthetic)
                .map(|parameter| (locate(parameter.span), range.span.start))
                .collect(),
        });
        block_scopes(&function.body, locate, &mut scopes);
    }
    let declarations = &checked.program.source_declarations;
    for declaration in declarations {
        if !matches!(
            declaration.kind,
            SourceDeclarationKind::TypeParameter | SourceDeclarationKind::Parameter
        ) {
            continue;
        }
        let parent = declarations
            .iter()
            .filter(|parent| {
                return matches!(
                    parent.kind,
                    SourceDeclarationKind::Function
                        | SourceDeclarationKind::Method
                        | SourceDeclarationKind::Struct
                        | SourceDeclarationKind::Union
                        | SourceDeclarationKind::Interface
                ) && parent.span.start <= declaration.name_span.start
                    && parent.span.end >= declaration.name_span.end;
            })
            .min_by_key(|parent| parent.span.end - parent.span.start);
        if let Some(parent) = parent {
            let range = locate(parent.span);
            scopes.push(Scope {
                range,
                declarations: vec![(
                    locate(declaration.name_span),
                    locate(declaration.name_span).span.end,
                )],
            });
        }
    }
    return scopes;
}
