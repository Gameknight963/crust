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
        return write!(formatter, "{}:{}", self.path.display(), self.diagnostic);
    }
}

impl std::error::Error for SourceError {}

pub struct SourceFile {
    pub path: PathBuf,
    pub text: String,
    offset: usize,
}

pub struct SourceSet {
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
        mut read: impl FnMut(&Path) -> io::Result<String>,
    ) -> Result<Self, SourceError> {
        let mut sources = Self {
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

    pub fn native_headers(&self) -> &[PathBuf] {
        return &self.native_headers;
    }

    pub fn check(&self) -> Result<(), SourceError> {
        return crate::frontend::run(|| self.check_inner()).map_err(|error| self.locate(error))?;
    }

    fn check_inner(&self) -> Result<(), SourceError> {
        let program =
            crate::parser::parse(self.tokens.clone()).map_err(|error| self.locate(error))?;
        crate::check::check(program).map_err(|error| self.locate(error))?;
        return Ok(());
    }

    pub fn compile(&self) -> Result<String, SourceError> {
        return crate::frontend::run(|| self.compile_inner())
            .map_err(|error| self.locate(error))?;
    }

    fn compile_inner(&self) -> Result<String, SourceError> {
        let program =
            crate::parser::parse(self.tokens.clone()).map_err(|error| self.locate(error))?;
        let checked = crate::check::check(program).map_err(|error| self.locate(error))?;
        return Ok(crate::emit_c::generate(&checked));
    }

    pub fn analyze(&self) -> Result<crate::analysis::Analysis, SourceError> {
        return self.analyze_inner(false);
    }

    pub(crate) fn analyze_completion(&self) -> Result<crate::analysis::Analysis, SourceError> {
        return self.analyze_inner(true);
    }

    fn analyze_inner(&self, completion: bool) -> Result<crate::analysis::Analysis, SourceError> {
        return crate::frontend::run(|| self.analyze_snapshot(completion))
            .map_err(|error| self.locate(error))?;
    }

    fn analyze_snapshot(&self, completion: bool) -> Result<crate::analysis::Analysis, SourceError> {
        let program =
            crate::parser::parse(self.tokens.clone()).map_err(|error| self.locate(error))?;
        let checked = if completion {
            crate::check::analyze_completion(program)
        } else {
            crate::check::check(program).map(|checked| checked.into_analysis())
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
        return Ok(crate::analysis::build(&checked, files, |mut span| {
            let file = self
                .files
                .iter()
                .rposition(|file| span.start >= file.offset)
                .unwrap();
            span.start -= self.files[file].offset;
            span.end = span
                .end
                .saturating_sub(self.files[file].offset)
                .min(self.files[file].text.len());
            crate::analysis::Location { file, span }
        }));
    }

    fn locate(&self, mut diagnostic: Diagnostic) -> SourceError {
        let file = self
            .files
            .iter()
            .rev()
            .find(|file| diagnostic.span.start >= file.offset)
            .unwrap();
        diagnostic.span.start = diagnostic.span.start.saturating_sub(file.offset);
        diagnostic.span.end = diagnostic
            .span
            .end
            .saturating_sub(file.offset)
            .min(file.text.len());
        return SourceError {
            path: file.path.clone(),
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
        let mut tokens = lexer::tokenize(&text).map_err(|diagnostic| SourceError {
            path: path.clone(),
            diagnostic,
        })?;
        let mut end = tokens.pop().unwrap();
        let offset = self.next_offset;
        self.next_offset += text.len() + 1;
        self.files.push(SourceFile {
            path: path.clone(),
            text,
            offset,
        });
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
