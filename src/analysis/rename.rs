use super::*;
use crate::lexer::{Token, tokenize};

impl Analysis {
    pub fn rename_target(&self, file: usize, offset: usize) -> Option<(&Symbol, Location)> {
        let definitions = self.definitions_at(file, offset);
        let [symbol] = definitions.as_slice() else {
            return None;
        };
        if !matches!(symbol.kind, SymbolKind::Variable | SymbolKind::Parameter) {
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
            return Err("select a local variable or explicit parameter to rename".to_owned());
        };
        if symbol.name == new_name {
            return Ok(Vec::new());
        }
        let locations = self.references_at(file, offset, true);
        let mut texts: Vec<_> = self.files.iter().map(|file| file.text.clone()).collect();
        for location in locations.iter().rev() {
            let span = location.span;
            let text = &mut texts[location.file];
            if text[span.start..span.end] != symbol.name {
                return Err("the selected binding has an ambiguous source spelling".to_owned());
            }
            text.replace_range(span.start..span.end, new_name);
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
                .analyze()
                .map_err(|error| format!("rename failed: {error}"))?
        } else {
            crate::analyze(&texts[0]).map_err(|error| format!("rename failed: {error}"))?
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
            let translate = |offset: usize| {
                let mut translated = offset;
                for edit in &locations {
                    if edit.file == location.file && edit.span.end <= offset {
                        translated =
                            translated + new_name.len() - (edit.span.end - edit.span.start);
                    }
                }
                return translated;
            };
            return (
                location.file,
                translate(location.span.start),
                translate(location.span.end),
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
        return Ok(locations
            .into_iter()
            .map(|location| TextEdit {
                location,
                new_text: new_name.to_owned(),
            })
            .collect());
    }
}
