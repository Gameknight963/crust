use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::ast::{Block, SourceDeclarationKind, Statement, StatementKind, Type, TypeDeclaration};
use crate::check::AnalyzedProgram;
use crate::diagnostic::Span;
mod completion;
mod members;
mod rename;
mod signatures;
pub use members::MemberAccess;
pub use signatures::{Signature, SignatureHelp, SignatureParameter};

#[derive(Debug, Clone)]
pub struct TextEdit {
    pub location: Location,
    pub new_text: String,
}

#[derive(Debug, Clone)]
pub struct File {
    pub path: Option<PathBuf>,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Location {
    pub file: usize,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Function,
    Method,
    Struct,
    Union,
    Enum,
    EnumMember,
    Field,
    Parameter,
    Variable,
    Global,
    Interface,
    Alias,
    TypeParameter,
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub location: Location,
    pub declaration: Location,
    pub range: Location,
    pub detail: String,
    pub container: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub location: Location,
    pub declaration: Option<Location>,
}

#[derive(Debug, Clone)]
pub struct TypeInfo {
    pub location: Location,
    pub description: String,
}

#[derive(Debug)]
pub struct Analysis {
    pub(crate) kind: crate::ProgramKind,
    pub files: Vec<File>,
    pub symbols: Vec<Symbol>,
    pub references: Vec<Reference>,
    pub types: Vec<TypeInfo>,
    completion_scopes: Vec<completion::Scope>,
    members: members::Index,
    signatures: signatures::Index,
    native_bindings: HashMap<Location, rename::NativeBinding>,
    scoped_enum_members: HashSet<Location>,
}

fn contains(location: Location, file: usize, offset: usize, include_end: bool) -> bool {
    return location.file == file
        && location.span.start <= offset
        && if include_end {
            offset <= location.span.end
        } else {
            offset < location.span.end
        };
}

impl Analysis {
    pub fn references_at(
        &self,
        file: usize,
        offset: usize,
        include_declarations: bool,
    ) -> Vec<Location> {
        let targets: HashSet<_> = self
            .definitions_at(file, offset)
            .iter()
            .map(|symbol| symbol.declaration)
            .collect();
        let mut locations: Vec<_> = self
            .references
            .iter()
            .filter(|reference| {
                reference
                    .declaration
                    .is_some_and(|target| targets.contains(&target))
            })
            .map(|reference| reference.location)
            .collect();
        if include_declarations {
            locations.extend(
                self.symbols
                    .iter()
                    .filter(|symbol| targets.contains(&symbol.declaration))
                    .map(|symbol| symbol.location),
            );
        }
        locations.sort_by_key(|location| (location.file, location.span.start, location.span.end));
        locations.dedup();
        return locations;
    }

    pub fn definitions_at(&self, file: usize, offset: usize) -> Vec<&Symbol> {
        for include_end in [false, true] {
            let shortest = self
                .references
                .iter()
                .filter(|reference| contains(reference.location, file, offset, include_end))
                .map(|reference| reference.location.span.end - reference.location.span.start)
                .min();
            if let Some(shortest) = shortest {
                let targets: HashSet<_> = self
                    .references
                    .iter()
                    .filter(|reference| {
                        contains(reference.location, file, offset, include_end)
                            && reference.location.span.end - reference.location.span.start
                                == shortest
                    })
                    .filter_map(|reference| reference.declaration)
                    .collect();
                return self
                    .symbols
                    .iter()
                    .filter(|symbol| targets.contains(&symbol.location))
                    .collect();
            }
            let definitions: Vec<_> = self
                .symbols
                .iter()
                .filter(|symbol| contains(symbol.location, file, offset, include_end))
                .collect();
            if !definitions.is_empty() {
                return definitions;
            }
        }
        return Vec::new();
    }

    pub fn type_at(&self, file: usize, offset: usize) -> Option<&TypeInfo> {
        return self
            .types
            .iter()
            .filter(|info| contains(info.location, file, offset, false))
            .min_by_key(|info| info.location.span.end - info.location.span.start);
    }
}

struct Builder<'a, F> {
    seen: HashSet<(Location, SymbolKind)>,
    analysis: Analysis,
    locate: &'a F,
}

impl<F: Fn(Span) -> Location> Builder<'_, F> {
    fn text(&self, span: Span) -> String {
        let location = (self.locate)(span);
        return self.analysis.files[location.file].text[location.span.start..location.span.end]
            .trim()
            .to_owned();
    }

    fn symbol(
        &mut self,
        name: String,
        kind: SymbolKind,
        span: Span,
        range: Span,
        detail: String,
        container: Option<String>,
    ) {
        let location = (self.locate)(span);
        if !self.seen.insert((location, kind)) {
            return;
        }
        self.analysis.symbols.push(Symbol {
            name,
            kind,
            location,
            declaration: location,
            range: (self.locate)(range),
            detail,
            container,
        });
    }

    fn block(&mut self, block: &Block) {
        for statement in &block.statements {
            self.statement(statement);
        }
    }

    fn statement(&mut self, statement: &Statement) {
        match &statement.kind {
            StatementKind::Local {
                name,
                name_span,
                declaration_span,
                ..
            } => {
                self.symbol(
                    name.clone(),
                    SymbolKind::Variable,
                    *name_span,
                    statement.span,
                    self.text(*declaration_span),
                    None,
                );
            }
            StatementKind::StaticLocal {
                name, name_span, ..
            } => {
                self.symbol(
                    name.clone(),
                    SymbolKind::Variable,
                    *name_span,
                    statement.span,
                    self.text(statement.span),
                    None,
                );
            }
            StatementKind::Block(block) | StatementKind::Unsafe(block) => self.block(block),
            StatementKind::If {
                then_block,
                else_block,
                ..
            } => {
                self.block(then_block);
                if let Some(block) = else_block {
                    self.block(block);
                }
            }
            StatementKind::While { body, step, .. } => {
                self.block(body);
                if let Some(step) = step {
                    self.statement(step);
                }
            }
            StatementKind::DoWhile { body, .. } => self.block(body),
            StatementKind::Switch { cases, .. } => {
                for case in cases {
                    self.block(&case.body);
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn build(
    checked: &AnalyzedProgram,
    files: Vec<File>,
    locate: impl Fn(Span) -> Location,
) -> Analysis {
    let signatures = signatures::Index::build(checked, &files, &locate);
    let native_bindings = rename::native_bindings(checked, &files, &locate);
    let mut builder = Builder {
        seen: HashSet::new(),
        analysis: Analysis {
            kind: checked.kind,
            files,
            symbols: Vec::new(),
            references: Vec::new(),
            types: Vec::new(),
            completion_scopes: completion::scopes(checked, &locate),
            members: members::Index::build(checked, &locate),
            signatures,
            native_bindings,
            scoped_enum_members: checked
                .program
                .enums
                .iter()
                .filter(|definition| definition.scoped)
                .flat_map(|definition| {
                    definition
                        .variants
                        .iter()
                        .map(|variant| locate(variant.span))
                })
                .collect(),
        },
        locate: &locate,
    };
    for declaration in &checked.program.source_declarations {
        let kind = match declaration.kind {
            SourceDeclarationKind::Function => SymbolKind::Function,
            SourceDeclarationKind::Struct => SymbolKind::Struct,
            SourceDeclarationKind::Union => SymbolKind::Union,
            SourceDeclarationKind::Interface => SymbolKind::Interface,
            SourceDeclarationKind::Alias => SymbolKind::Alias,
            SourceDeclarationKind::TypeParameter => SymbolKind::TypeParameter,
            SourceDeclarationKind::Method => SymbolKind::Method,
            SourceDeclarationKind::Parameter => SymbolKind::Parameter,
        };
        builder.symbol(
            builder.text(declaration.name_span),
            kind,
            declaration.name_span,
            declaration.span,
            builder.text(declaration.header_span),
            declaration.container.clone(),
        );
    }
    for global in &checked.program.globals {
        if global.function_local {
            continue;
        }
        builder.symbol(
            builder.text(global.name_span),
            SymbolKind::Global,
            global.name_span,
            global.span,
            builder.text(global.span).trim_end_matches(';').to_owned(),
            None,
        );
    }
    for (id, (name, span)) in checked.program.struct_names.iter().enumerate() {
        if !checked.types.structs[id].complete {
            let kind = if checked.types.structs[id].is_union || checked.types.structs[id].tagged {
                SymbolKind::Union
            } else {
                SymbolKind::Struct
            };
            let tag = if kind == SymbolKind::Union {
                "union"
            } else {
                "struct"
            };
            let mut detail = format!("{tag} {name}");
            if let Some(native) = &checked.types.structs[id].native_name {
                detail.push_str(&format!(" = \"{native}\""));
            }
            builder.symbol(name.clone(), kind, *span, *span, detail, None);
        }
    }
    for definition in &checked.program.structs {
        let name = builder.text(definition.name_span);
        let kind = if checked.types.structs[definition.id].is_union
            || checked.types.structs[definition.id].tagged
        {
            SymbolKind::Union
        } else {
            SymbolKind::Struct
        };
        builder.symbol(
            name.clone(),
            kind,
            definition.name_span,
            definition.span,
            builder.text(definition.header_span),
            None,
        );
        for field in &definition.fields {
            builder.symbol(
                field.name.clone(),
                SymbolKind::Field,
                field.span,
                field.declaration_span,
                builder.text(field.declaration_span),
                Some(name.clone()),
            );
        }
    }
    for definition in &checked.program.enums {
        let name = builder.text(definition.name_span);
        builder.symbol(
            name.clone(),
            SymbolKind::Enum,
            definition.name_span,
            definition.span,
            builder.text(definition.header_span),
            None,
        );
        for variant in &definition.variants {
            builder.symbol(
                variant.name.clone(),
                SymbolKind::EnumMember,
                variant.span,
                variant.span,
                format!("{name}::{} = {}", variant.name, variant.value),
                Some(name.clone()),
            );
        }
    }
    for function in &checked.program.functions {
        if function.name.starts_with('?') {
            continue;
        }
        let owner = function
            .name
            .strip_prefix('@')
            .or_else(|| function.name.strip_prefix('#'))
            .map(|name| name.split_once('_').unwrap().0)
            .or_else(|| function.name.strip_prefix('~'))
            .map(|id| id.parse::<usize>().unwrap());
        let mut name = builder.text(function.name_span);
        if function.name.starts_with('~') {
            name = format!("~{name}");
        }
        let kind = if owner.is_some() {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };
        let container = owner
            .and_then(|id| {
                checked
                    .program
                    .structs
                    .iter()
                    .find(|definition| definition.id == id)
            })
            .map(|definition| builder.text(definition.name_span));
        let header = Span {
            end: function.body.span.start,
            ..function.span
        };
        builder.symbol(
            name,
            kind,
            function.name_span,
            function.span,
            builder.text(header),
            container,
        );
        for parameter in &function.parameters {
            if !parameter.synthetic {
                builder.symbol(
                    parameter.name.clone(),
                    SymbolKind::Parameter,
                    parameter.span,
                    parameter.declaration_span,
                    builder.text(parameter.declaration_span),
                    None,
                );
            }
        }
        builder.block(&function.body);
    }
    let mut canonical = HashMap::new();
    let mut globals = HashMap::new();
    for global in &checked.program.globals {
        let location = locate(global.name_span);
        let declaration = *globals.entry(&global.name).or_insert(location);
        canonical.insert(location, declaration);
    }
    let mut functions = HashMap::new();
    for function in &checked.program.functions {
        let location = locate(function.name_span);
        let declaration = *functions.entry(&function.name).or_insert(location);
        canonical.insert(location, declaration);
    }
    let mut aliases = HashMap::new();
    for declaration in &checked.program.source_declarations {
        if matches!(declaration.kind, SourceDeclarationKind::Alias) {
            let location = locate(declaration.name_span);
            let name = builder.text(declaration.name_span);
            let target = *aliases.entry(name).or_insert(location);
            canonical.insert(location, target);
        }
    }
    for symbol in &mut builder.analysis.symbols {
        if let Some(declaration) = canonical.get(&symbol.location) {
            symbol.declaration = *declaration;
        }
    }
    let mut descriptions: HashMap<Span, HashSet<Type>> = HashMap::new();
    for reference in &checked.program.type_references {
        let declaration = match reference.declaration {
            Some(TypeDeclaration::Source(span)) => Some(span),
            Some(TypeDeclaration::Aggregate(id)) => checked
                .program
                .structs
                .iter()
                .find(|definition| definition.id == id)
                .map(|definition| definition.name_span)
                .or_else(|| checked.program.struct_names.get(id).map(|(_, span)| *span)),
            Some(TypeDeclaration::Enum(id)) => checked
                .program
                .enums
                .iter()
                .find(|definition| definition.id == id)
                .map(|definition| definition.name_span),
            None => None,
        };
        builder.analysis.references.push(Reference {
            location: locate(reference.span),
            declaration: declaration.map(&locate),
        });
    }
    for (span, ty) in &checked.program.type_annotations {
        descriptions.entry(*span).or_default().insert(*ty);
    }
    for function in checked.analyses.values() {
        for (span, declaration) in &function.symbol_uses {
            builder.analysis.references.push(Reference {
                location: locate(*span),
                declaration: Some(locate(*declaration)),
            });
        }
        for (span, ty) in &function.expression_types {
            descriptions.entry(*span).or_default().insert(*ty);
        }
    }
    for (span, descriptions) in descriptions {
        // Replayed template bodies can have different concrete types at the
        // same source position. Their source declarations remain available.
        if descriptions.len() == 1 {
            builder.analysis.types.push(TypeInfo {
                location: locate(span),
                description: checked
                    .types
                    .describe(descriptions.into_iter().next().unwrap()),
            });
        }
    }
    for reference in &mut builder.analysis.references {
        if let Some(target) = reference.declaration
            && let Some(declaration) = canonical.get(&target)
        {
            reference.declaration = Some(*declaration);
        }
    }
    builder.analysis.symbols.sort_by_key(|symbol| {
        (
            symbol.location.file,
            symbol.location.span.start,
            symbol.location.span.end,
        )
    });
    builder.analysis.references.sort_by_key(|reference| {
        (
            reference.location.file,
            reference.location.span.start,
            reference.location.span.end,
            reference
                .declaration
                .map(|location| location.file)
                .unwrap_or(usize::MAX),
            reference
                .declaration
                .map(|location| location.span.start)
                .unwrap_or(0),
        )
    });
    builder.analysis.references.dedup();
    builder.analysis.types.sort_by_key(|info| {
        (
            info.location.file,
            info.location.span.start,
            info.location.span.end,
        )
    });
    return builder.analysis;
}
