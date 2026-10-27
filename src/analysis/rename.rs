use super::*;
use crate::lexer::{Token, tokenize};

#[derive(Debug)]
pub(super) struct NativeBinding {
    name: String,
    alias: Option<Location>,
}

pub(super) fn native_bindings(
    checked: &AnalyzedProgram,
    files: &[File],
    locate: &impl Fn(Span) -> Location,
) -> HashMap<Location, NativeBinding> {
    let tokens: Vec<_> = files
        .iter()
        .map(|file| tokenize(&file.text).unwrap_or_default())
        .collect();
    let mut bindings = HashMap::new();
    let declarations = checked
        .program
        .functions
        .iter()
        .filter(|function| function.link_name.is_some())
        .map(|function| {
            (
                function.name_span,
                Span {
                    end: function.body.span.start,
                    ..function.span
                },
                function.link_name.as_ref().unwrap(),
            )
        })
        .chain(checked.program.globals.iter().map(|global| {
            (
                global.name_span,
                Span {
                    end: global.span.end - 1,
                    ..global.span
                },
                &global.link_name,
            )
        }));
    for (name, range, native) in declarations {
        let range = locate(range);
        let file_tokens = &tokens[range.file];
        let start = file_tokens.partition_point(|token| token.span.start < range.span.start);
        let end = file_tokens.partition_point(|token| token.span.start < range.span.end);
        let tokens = &file_tokens[start..end];
        let alias = if tokens.iter().any(|token| token.kind == Token::Equal) {
            None
        } else {
            let last = tokens.last().unwrap();
            Some(Location {
                file: range.file,
                span: Span {
                    start: last.span.end,
                    end: last.span.end,
                    column: last.span.column + last.span.end - last.span.start,
                    ..last.span
                },
            })
        };
        bindings.insert(
            locate(name),
            NativeBinding {
                name: native.clone(),
                alias,
            },
        );
    }
    for definition in &checked.program.enums {
        let Some(name) = &definition.native_name else {
            continue;
        };
        let location = locate(definition.name_span);
        let file_tokens = &tokens[location.file];
        let following = file_tokens
            .get(file_tokens.partition_point(|token| token.span.start < location.span.end));
        let alias = if following.is_some_and(|token| token.kind == Token::Equal) {
            None
        } else {
            Some(Location {
                file: location.file,
                span: Span {
                    start: location.span.end,
                    end: location.span.end,
                    column: location.span.column + location.span.end - location.span.start,
                    ..location.span
                },
            })
        };
        bindings.insert(
            location,
            NativeBinding {
                name: name.clone(),
                alias,
            },
        );
    }
    for (id, (name, span)) in checked.program.struct_names.iter().enumerate() {
        if !checked.types.structs[id].complete {
            bindings.insert(
                locate(*span),
                NativeBinding {
                    name: name.clone(),
                    alias: None,
                },
            );
        }
    }
    return bindings;
}

impl Analysis {
    pub fn rename_target(&self, file: usize, offset: usize) -> Option<(&Symbol, Location)> {
        let definitions = self.definitions_at(file, offset);
        let [symbol] = definitions.as_slice() else {
            return None;
        };
        if !matches!(
            symbol.kind,
            SymbolKind::Variable
                | SymbolKind::Parameter
                | SymbolKind::Function
                | SymbolKind::Method
                | SymbolKind::Alias
                | SymbolKind::Interface
                | SymbolKind::TypeParameter
                | SymbolKind::Enum
                | SymbolKind::Struct
                | SymbolKind::Union
                | SymbolKind::Global
        ) || (symbol.kind == SymbolKind::Function && symbol.name == "main")
            || (symbol.kind == SymbolKind::Parameter && symbol.name == "this")
            || (symbol.kind == SymbolKind::Method
                && (symbol.name.starts_with('~') || symbol.detail.trim_start().starts_with('~')))
            || (matches!(symbol.kind, SymbolKind::Struct | SymbolKind::Union)
                && self.native_bindings.contains_key(&symbol.declaration))
        {
            return None;
        }
        let location = self
            .references_at(file, offset, true)
            .into_iter()
            .find(|location| contains(*location, file, offset, false))
            .or_else(|| {
                self.references_at(file, offset, true)
                    .into_iter()
                    .find(|location| contains(*location, file, offset, true))
            })?;
        if self.files[location.file].text[location.span.start..location.span.end] != symbol.name {
            return None;
        }
        return Some((symbol, location));
    }

    pub fn rename(
        &self,
        file: usize,
        offset: usize,
        new_name: &str,
    ) -> Result<Vec<TextEdit>, String> {
        let tokens = tokenize(new_name).map_err(|error| error.to_string())?;
        if tokens.len() != 2
            || !matches!(&tokens[0].kind, Token::Identifier(name) if name == new_name)
        {
            return Err("the new name must be an identifier".to_owned());
        }
        let Some((symbol, _)) = self.rename_target(file, offset) else {
            return Err(
                "select a local, parameter, type parameter, function, method, typedef, interface, type, or external variable to rename"
                    .to_owned(),
            );
        };
        if symbol.name == new_name {
            return Ok(Vec::new());
        }
        self.check_rename_coverage(symbol, new_name)?;
        let locations = self.references_at(file, offset, true);
        let mut edits = Vec::new();
        for location in locations {
            if matches!(
                symbol.kind,
                SymbolKind::Interface | SymbolKind::Struct | SymbolKind::Union
            ) && &self.files[location.file].text[location.span.start..location.span.end]
                == "Self"
            {
                continue;
            }
            if self.files[location.file].text[location.span.start..location.span.end] != symbol.name
            {
                return Err("the selected binding has an ambiguous source spelling".to_owned());
            }
            edits.push(TextEdit {
                location,
                new_text: new_name.to_owned(),
            });
        }
        if matches!(
            symbol.kind,
            SymbolKind::Function | SymbolKind::Global | SymbolKind::Enum
        ) {
            for declaration in self
                .symbols
                .iter()
                .filter(|declaration| declaration.declaration == symbol.declaration)
            {
                if let Some(binding) = self.native_bindings.get(&declaration.location)
                    && let Some(location) = binding.alias
                {
                    edits.push(TextEdit {
                        location,
                        new_text: format!(" = \"{}\"", binding.name),
                    });
                }
            }
        }
        edits.sort_by_key(|edit| {
            (
                edit.location.file,
                edit.location.span.start,
                edit.location.span.end,
            )
        });
        self.validate_rename_edits(&edits)?;
        return Ok(edits);
    }

    fn check_rename_coverage(&self, symbol: &Symbol, new_name: &str) -> Result<(), String> {
        let global = matches!(
            symbol.kind,
            SymbolKind::Function
                | SymbolKind::Method
                | SymbolKind::Alias
                | SymbolKind::Interface
                | SymbolKind::Enum
                | SymbolKind::Struct
                | SymbolKind::Union
                | SymbolKind::Global
        );
        let scope = self
            .symbols
            .iter()
            .filter(|parent| {
                (matches!(parent.kind, SymbolKind::Function | SymbolKind::Method)
                    || (symbol.kind == SymbolKind::TypeParameter
                        && matches!(parent.kind, SymbolKind::Struct | SymbolKind::Union)))
                    && contains(
                        parent.range,
                        symbol.location.file,
                        symbol.location.span.start,
                        false,
                    )
            })
            .min_by_key(|parent| parent.range.span.end - parent.range.span.start)
            .map(|parent| parent.range);
        let bound: HashSet<_> = self
            .symbols
            .iter()
            .map(|symbol| symbol.location)
            .chain(self.references.iter().map(|reference| reference.location))
            .collect();
        for (file, source) in self.files.iter().enumerate() {
            let tokens = tokenize(&source.text).map_err(|error| error.to_string())?;
            for (index, token) in tokens.iter().enumerate() {
                let Token::Identifier(name) = &token.kind else {
                    continue;
                };
                if name != &symbol.name && name != new_name {
                    continue;
                }
                let location = Location {
                    file,
                    span: token.span,
                };
                if matches!(symbol.kind, SymbolKind::Function | SymbolKind::Global)
                    && index > 0
                    && matches!(
                        tokens[index - 1].kind,
                        Token::Dot | Token::Arrow | Token::ColonColon
                    )
                {
                    continue;
                }
                if !global
                    && !scope.is_some_and(|scope| contains(scope, file, token.span.start, false))
                {
                    continue;
                }
                let contextual = (name == "header"
                    && index > 0
                    && tokens[index - 1].kind == Token::Extern
                    && tokens
                        .get(index + 1)
                        .is_some_and(|token| matches!(token.kind, Token::StringLiteral(_))))
                    || (name == "dllimport"
                        && index >= 2
                        && tokens[index - 1].kind == Token::LeftParen
                        && tokens[index - 2].kind == Token::Declspec);
                if !contextual && !bound.contains(&location) {
                    return Err(format!(
                        "rename cannot resolve every occurrence of '{name}'"
                    ));
                }
            }
        }
        return Ok(());
    }

    pub(crate) fn validate_rename_edits(&self, edits: &[TextEdit]) -> Result<(), String> {
        let mut texts: Vec<_> = self.files.iter().map(|file| file.text.clone()).collect();
        for edit in edits.iter().rev() {
            let text = texts
                .get_mut(edit.location.file)
                .ok_or("rename edit references an unknown source file")?;
            let span = edit.location.span;
            if text.get(span.start..span.end).is_none() {
                return Err("rename edit is outside its source file".to_owned());
            }
            text.replace_range(span.start..span.end, &edit.new_text);
        }
        let candidate = if let Some(entry) = &self.files[0].path {
            let sources = crate::source::SourceSet::load_with(entry, |path| {
                let file = self
                    .files
                    .iter()
                    .position(|file| file.path.as_deref() == Some(path));
                return file.map(|file| texts[file].clone()).ok_or_else(|| {
                    std::io::Error::other("source file is absent from the analysis snapshot")
                });
            })
            .map_err(|error| format!("rename failed: {error}"))?;
            sources
                .analyze_with_kind(self.kind)
                .map_err(|error| format!("rename failed: {error}"))?
        } else {
            crate::analyze_with_kind(&texts[0], self.kind)
                .map_err(|error| format!("rename failed: {error}"))?
        };
        if self
            .files
            .iter()
            .map(|file| &file.path)
            .ne(candidate.files.iter().map(|file| &file.path))
        {
            return Err("rename changed the source dependency graph".to_owned());
        }
        let shifted = |location: Location| {
            let translate = |offset: usize, end: bool| {
                let mut translated = offset;
                for edit in edits {
                    let span = edit.location.span;
                    if edit.location.file == location.file
                        && (span.end < offset
                            || (span.end == offset && (!end || span.start != span.end)))
                    {
                        translated = translated + edit.new_text.len() - (span.end - span.start);
                    }
                }
                return translated;
            };
            return (
                location.file,
                translate(location.span.start, false),
                translate(location.span.end, true),
            );
        };
        let unchanged =
            |location: Location| (location.file, location.span.start, location.span.end);
        let expected: std::collections::BTreeSet<_> = self
            .references
            .iter()
            .map(|reference| {
                (
                    shifted(reference.location),
                    reference.declaration.map(shifted),
                )
            })
            .collect();
        let actual: std::collections::BTreeSet<_> = candidate
            .references
            .iter()
            .map(|reference| {
                (
                    unchanged(reference.location),
                    reference.declaration.map(unchanged),
                )
            })
            .collect();
        if expected != actual {
            return Err("rename would change which declaration an identifier refers to".to_owned());
        }
        let native: HashMap<_, _> = candidate
            .native_bindings
            .iter()
            .map(|(location, binding)| (unchanged(*location), &binding.name))
            .collect();
        for (location, binding) in &self.native_bindings {
            if native.get(&shifted(*location)) != Some(&&binding.name) {
                return Err("rename would change a native C symbol".to_owned());
            }
        }
        return Ok(());
    }
}
