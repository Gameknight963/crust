use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::diagnostic::{Diagnostic, Span};
use crate::lexer::{self, SpannedToken, Token};

#[derive(Debug)]
pub struct SourceError {
    pub path: PathBuf,
    pub diagnostic: Diagnostic,
}

impl fmt::Display for SourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(
            formatter,
            "{}:{}",
            display_path(&self.path),
            self.diagnostic
        );
    }
}

impl std::error::Error for SourceError {}

pub(crate) fn display_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    return path
        .strip_prefix(r"\\?\UNC\")
        .map(|path| format!(r"\\{path}"))
        .unwrap_or_else(|| path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned());
}

pub struct SourceFile {
    pub path: PathBuf,
    pub text: String,
    offset: usize,
    prepared: Option<crate::preprocess::Prepared>,
}

pub struct SourceSet {
    native: crate::native::Options,
    native_headers: Vec<PathBuf>,
    files: Vec<SourceFile>,
    tokens: Vec<SpannedToken>,
    loaded: HashSet<PathBuf>,
    next_offset: usize,
}

// Existing paths use filesystem identity. New editor documents use a normalized
// absolute path so unsaved imports and their disk counterparts share a key.
pub fn identity(path: &Path) -> io::Result<PathBuf> {
    if let Ok(canonical) = path.canonicalize() {
        return Ok(canonical);
    }
    let absolute = std::path::absolute(path)?;
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    return Ok(normalized);
}

pub(crate) fn header_name(path: &Path) -> io::Result<String> {
    let name = path
        .to_str()
        .ok_or_else(|| io::Error::other("native header paths require UTF-8"))?;
    #[cfg(windows)]
    let name = if let Some(unc) = name.strip_prefix(r"\\?\UNC\") {
        format!("//{}", unc.replace('\\', "/"))
    } else {
        name.strip_prefix(r"\\?\")
            .unwrap_or(name)
            .replace('\\', "/")
    };
    #[cfg(not(windows))]
    let name = name.to_owned();
    if name.is_empty() || name.contains(['\0', '\n', '\r', '"']) {
        return Err(io::Error::other(
            "native header path is not representable as a C include",
        ));
    }
    return Ok(name);
}

impl SourceSet {
    pub fn load(entry: &Path) -> Result<Self, SourceError> {
        return Self::load_with(entry, |path| fs::read_to_string(path));
    }

    pub fn load_with(
        entry: &Path,
        read: impl FnMut(&Path) -> io::Result<String>,
    ) -> Result<Self, SourceError> {
        return Self::load_with_options(entry, &crate::native::Options::default(), read);
    }

    pub fn load_with_options(
        entry: &Path,
        native: &crate::native::Options,
        mut read: impl FnMut(&Path) -> io::Result<String>,
    ) -> Result<Self, SourceError> {
        let mut sources = Self {
            native: native.clone(),
            native_headers: Vec::new(),
            files: Vec::new(),
            tokens: Vec::new(),
            loaded: HashSet::new(),
            next_offset: 0,
        };
        sources.load_file(entry, None, 0, &mut read)?;
        return Ok(sources);
    }

    pub fn files(&self) -> &[SourceFile] {
        return &self.files;
    }

    /// Expanded Crust tokens with positions in their original source files.
    pub fn tokens(&self) -> Vec<(PathBuf, SpannedToken)> {
        return self
            .tokens
            .iter()
            .filter(|token| !matches!(token.kind, Token::NativeCode(_)))
            .map(|token| {
                let location = self.location(token.span);
                let mut token = token.clone();
                token.span = location.span;
                return (self.files[location.file].path.clone(), token);
            })
            .collect();
    }

    pub fn native_headers(&self) -> &[PathBuf] {
        return &self.native_headers;
    }

    pub fn check(&self) -> Result<(), SourceError> {
        return self.check_with_kind(crate::ProgramKind::Executable);
    }

    pub fn check_with_kind(&self, kind: crate::ProgramKind) -> Result<(), SourceError> {
        return crate::frontend::run(|| self.check_inner(kind))
            .map_err(|error| self.locate(error))?;
    }

    fn check_inner(&self, kind: crate::ProgramKind) -> Result<(), SourceError> {
        let program =
            crate::parser::parse(self.tokens.clone()).map_err(|error| self.locate(error))?;
        crate::check::check_with_kind(program, kind).map_err(|error| self.locate(error))?;
        return Ok(());
    }

    pub fn compile(&self) -> Result<String, SourceError> {
        return self.compile_with_kind(crate::ProgramKind::Executable);
    }

    pub fn emit_header(&self, kind: crate::ProgramKind) -> Result<String, SourceError> {
        return crate::frontend::run(|| {
            let program =
                crate::parser::parse(self.tokens.clone()).map_err(|error| self.locate(error))?;
            let checked =
                crate::check::check_with_kind(program, kind).map_err(|error| self.locate(error))?;
            return crate::header::generate(&checked).map_err(|error| self.locate(error));
        })
        .map_err(|error| self.locate(error))?;
    }

    pub fn compile_with_kind(&self, kind: crate::ProgramKind) -> Result<String, SourceError> {
        return crate::frontend::run(|| self.compile_inner(kind))
            .map_err(|error| self.locate(error))?;
    }

    fn compile_inner(&self, kind: crate::ProgramKind) -> Result<String, SourceError> {
        let program =
            crate::parser::parse(self.tokens.clone()).map_err(|error| self.locate(error))?;
        let checked =
            crate::check::check_with_kind(program, kind).map_err(|error| self.locate(error))?;
        let sources = self
            .files
            .iter()
            .flat_map(|file| {
                if let Some(prepared) = &file.prepared {
                    let mut previous = None;
                    return prepared
                        .origins
                        .iter()
                        .filter(|origin| {
                            let key = (&origin.path, origin.span.line);
                            if previous == Some(key) {
                                return false;
                            }
                            previous = Some(key);
                            return true;
                        })
                        .map(|origin| {
                            crate::emit_c::Source::mapped(
                                file.offset + origin.start,
                                display_path(&origin.path),
                                origin.span.line,
                            )
                        })
                        .collect::<Vec<_>>();
                }
                return vec![crate::emit_c::Source::new(
                    file.offset,
                    display_path(&file.path),
                    &file.text,
                )];
            })
            .collect::<Vec<_>>();
        return Ok(crate::emit_c::generate_with_sources(&checked, &sources));
    }

    pub fn analyze(&self) -> Result<crate::analysis::Analysis, SourceError> {
        return self.analyze_with_kind(crate::ProgramKind::Executable);
    }

    pub fn analyze_with_kind(
        &self,
        kind: crate::ProgramKind,
    ) -> Result<crate::analysis::Analysis, SourceError> {
        return self.analyze_inner(false, kind);
    }

    pub(crate) fn analyze_completion_with_kind(
        &self,
        kind: crate::ProgramKind,
    ) -> Result<crate::analysis::Analysis, SourceError> {
        return self.analyze_inner(true, kind);
    }

    fn analyze_inner(
        &self,
        completion: bool,
        kind: crate::ProgramKind,
    ) -> Result<crate::analysis::Analysis, SourceError> {
        return crate::frontend::run(|| self.analyze_snapshot(completion, kind))
            .map_err(|error| self.locate(error))?;
    }

    fn analyze_snapshot(
        &self,
        completion: bool,
        kind: crate::ProgramKind,
    ) -> Result<crate::analysis::Analysis, SourceError> {
        let program =
            crate::parser::parse(self.tokens.clone()).map_err(|error| self.locate(error))?;
        let checked = if completion {
            crate::check::analyze_completion_with_kind(program, kind)
        } else {
            crate::check::check_with_kind(program, kind).map(|checked| checked.into_analysis())
        }
        .map_err(|error| self.locate(error))?;
        let files = self
            .files
            .iter()
            .map(|file| crate::analysis::File {
                path: Some(file.path.clone()),
                text: file.text.clone(),
            })
            .collect();
        let mut analysis = crate::analysis::build(&checked, files, |span| self.location(span));
        for symbol in &mut analysis.symbols {
            if symbol.detail.is_empty() {
                let text = &analysis.files[symbol.location.file].text;
                let start = text[..symbol.location.span.start.min(text.len())]
                    .rfind('\n')
                    .map_or(0, |offset| offset + 1);
                symbol.detail = text[start..]
                    .split(['{', ';', '\n'])
                    .next()
                    .unwrap_or(&symbol.name)
                    .trim()
                    .to_owned();
            }
        }
        analysis.native_options = Some(self.native.clone());
        for file in &self.files {
            if let Some(prepared) = &file.prepared {
                for origin in prepared.origins.iter().filter(|origin| origin.generated) {
                    analysis.macro_expansions.push(self.location(Span {
                        start: file.offset + origin.start,
                        end: file.offset + origin.end,
                        ..origin.span
                    }));
                }
            }
        }
        return Ok(analysis);
    }

    fn location(&self, mut span: Span) -> crate::analysis::Location {
        let file = self
            .files
            .iter()
            .rposition(|file| span.start >= file.offset)
            .unwrap();
        span.start -= self.files[file].offset;
        span.end = span.end.saturating_sub(self.files[file].offset);
        if let Some(prepared) = &self.files[file].prepared
            && let Some((path, mapped)) = prepared.locate(span)
        {
            let file = self
                .files
                .iter()
                .position(|file| file.path == path)
                .unwrap_or(file);
            return crate::analysis::Location { file, span: mapped };
        }
        span.end = span.end.min(self.files[file].text.len());
        return crate::analysis::Location { file, span };
    }

    fn locate(&self, mut diagnostic: Diagnostic) -> SourceError {
        let location = self.location(diagnostic.span);
        diagnostic.span = location.span;
        return SourceError {
            path: self.files[location.file].path.clone(),
            diagnostic,
        };
    }

    fn load_file(
        &mut self,
        path: &Path,
        origin: Option<Span>,
        import_depth: usize,
        read: &mut impl FnMut(&Path) -> io::Result<String>,
    ) -> Result<(), SourceError> {
        let fallback = Span {
            start: 0,
            end: 0,
            line: 1,
            column: 1,
        };
        let path = identity(path).map_err(|error| SourceError {
            path: path.to_owned(),
            diagnostic: Diagnostic::new(fallback, error.to_string()),
        })?;
        if !self.loaded.insert(path.clone()) {
            return Ok(());
        }
        if import_depth >= 128 || self.files.len() >= 1024 {
            return Err(self.locate(Diagnostic::new(
                origin.unwrap_or(fallback),
                "source import limit exceeded",
            )));
        }
        let text = read(&path).map_err(|error| {
            let diagnostic = Diagnostic::new(
                origin.unwrap_or(fallback),
                format!("could not read {}: {error}", path.display()),
            );
            return if origin.is_some() {
                self.locate(diagnostic)
            } else {
                SourceError {
                    path: path.clone(),
                    diagnostic,
                }
            };
        })?;
        let mut prepared = if crate::preprocess::needed(&text) || !self.native.defines.is_empty() {
            Some(crate::preprocess::prepare(
                &path,
                &text,
                &self.native,
                read,
            )?)
        } else {
            None
        };
        let mut tokens = if let Some(prepared) = &mut prepared {
            std::mem::take(&mut prepared.tokens)
        } else {
            lexer::tokenize(&text).map_err(|diagnostic| SourceError {
                path: path.clone(),
                diagnostic,
            })?
        };
        let mut end = tokens.pop().unwrap();
        let offset = self.next_offset;
        self.next_offset += prepared.as_ref().map_or(text.len(), |p| p.text.len()) + 1;
        let inputs = prepared
            .as_ref()
            .map(|p| p.inputs.clone())
            .unwrap_or_default();
        self.files.push(SourceFile {
            path: path.clone(),
            text,
            offset,
            prepared,
        });
        for (input, content) in inputs {
            if self.files.iter().any(|file| file.path == input) {
                continue;
            }
            self.native_headers.push(input.clone());
            self.files.push(SourceFile {
                path: input,
                text: content.clone(),
                offset: self.next_offset,
                prepared: None,
            });
            self.next_offset += content.len() + 1;
        }
        for token in &mut tokens {
            token.span.start += offset;
            token.span.end += offset;
        }
        end.span.start += offset;
        end.span.end += offset;
        let mut imports = Vec::new();
        let mut position = 0;
        let mut depth = 0usize;
        let mut declaration_start = true;
        while position < tokens.len() {
            let token = &tokens[position];
            if declaration_start
                && token.kind == Token::Extern
                && matches!(tokens.get(position + 1).map(|token| &token.kind), Some(Token::Identifier(name)) if name == "header")
                && let Some(SpannedToken {
                    kind: Token::StringLiteral(bytes),
                    span,
                }) = tokens.get(position + 2)
            {
                let name = std::str::from_utf8(bytes).map_err(|_| {
                    self.locate(Diagnostic::new(*span, "native header paths require UTF-8"))
                })?;
                if name.contains('\0') {
                    return Err(self.locate(Diagnostic::new(
                        *span,
                        "native header paths cannot contain null bytes",
                    )));
                }
                if !tokens
                    .get(position + 3)
                    .is_some_and(|token| token.kind == Token::Semicolon)
                {
                    return Err(self.locate(Diagnostic::new(
                        *span,
                        "expected ';' after native header reference",
                    )));
                }
                let header = path
                    .parent()
                    .unwrap()
                    .join(name)
                    .canonicalize()
                    .map_err(|error| {
                        self.locate(Diagnostic::new(
                            *span,
                            format!("could not open native header '{name}': {error}"),
                        ))
                    })?;
                if !header.is_file() {
                    return Err(self.locate(Diagnostic::new(
                        *span,
                        "a native header reference requires a file",
                    )));
                }
                let name = header_name(&header)
                    .map_err(|error| self.locate(Diagnostic::new(*span, error.to_string())))?;
                if !self.native_headers.contains(&header) {
                    self.native_headers.push(header);
                }
                self.tokens
                    .extend_from_slice(&tokens[position..position + 2]);
                let mut token = tokens[position + 2].clone();
                token.kind = Token::StringLiteral(name.into_bytes());
                self.tokens.push(token);
                self.tokens.push(tokens[position + 3].clone());
                position += 4;
                continue;
            }
            if token.kind == Token::Import && depth == 0 {
                if !declaration_start {
                    return Err(self.locate(Diagnostic::new(
                        token.span,
                        "imports must be top-level declarations",
                    )));
                }
                let Some(SpannedToken {
                    kind: Token::StringLiteral(bytes),
                    ..
                }) = tokens.get(position + 1)
                else {
                    return Err(
                        self.locate(Diagnostic::new(token.span, "expected a quoted import path"))
                    );
                };
                let name = std::str::from_utf8(bytes)
                    .ok()
                    .filter(|name| !name.is_empty() && !name.contains('\0'))
                    .ok_or_else(|| {
                        self.locate(Diagnostic::new(
                            token.span,
                            "import paths must be nonempty UTF-8 without null bytes",
                        ))
                    })?;
                if !tokens
                    .get(position + 2)
                    .is_some_and(|token| token.kind == Token::Semicolon)
                {
                    return Err(
                        self.locate(Diagnostic::new(token.span, "expected ';' after import"))
                    );
                }
                let imported = Path::new(name);
                if imported
                    .components()
                    .any(|part| matches!(part, Component::Prefix(_) | Component::RootDir))
                {
                    return Err(self.locate(Diagnostic::new(
                        token.span,
                        "import paths must be relative to the source file",
                    )));
                }
                imports.push((path.parent().unwrap().join(imported), token.span));
                position += 3;
                continue;
            }
            match token.kind {
                Token::LeftBrace => depth += 1,
                Token::RightBrace => depth = depth.saturating_sub(1),
                _ => {}
            }
            declaration_start =
                depth == 0 && matches!(token.kind, Token::Semicolon | Token::RightBrace);
            self.tokens.push(token.clone());
            position += 1;
        }
        self.tokens.push(end);
        for (imported, span) in imports {
            self.load_file(&imported, Some(span), import_depth + 1, read)?;
        }
        return Ok(());
    }
}
