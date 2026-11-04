use super::*;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub(super) struct Snapshot {
    directory: PathBuf,
    pub entry: PathBuf,
    pub inputs: BTreeMap<PathBuf, String>,
    staged: BTreeMap<PathBuf, PathBuf>,
    pub root_input: String,
    pub epoch: std::ffi::OsString,
}

pub(super) struct Import {
    pub span: Span,
    pub argument: std::ops::Range<usize>,
    pub literal: Option<String>,
    pub library: Option<String>,
}

pub(super) fn imports(text: &str) -> Vec<Import> {
    let tokens = lexer::discovery_tokens(text);
    let mut result = Vec::new();
    let mut previous = 0;
    for (index, token) in tokens.iter().enumerate() {
        if token.kind == Token::Import && token.span.start >= previous {
            let Some(end) = tokens[index + 1..]
                .iter()
                .find(|t| t.kind == Token::Semicolon)
            else {
                continue;
            };
            let literal = match tokens.get(index + 1).map(|t| &t.kind) {
                Some(Token::StringLiteral(bytes)) => String::from_utf8(bytes.clone()).ok(),
                Some(Token::LibraryPath(name)) => crate::source::library_path(name)
                    .ok()
                    .map(|path| crate::source::display_path(&path)),
                _ => None,
            };
            previous = end.span.end;
            result.push(Import {
                span: Span {
                    end: end.span.end,
                    ..token.span
                },
                argument: token.span.end..end.span.start,
                literal,
                library: match tokens.get(index + 1).map(|token| &token.kind) {
                    Some(Token::LibraryPath(name)) => Some(name.clone()),
                    _ => None,
                },
            });
        }
    }
    return result;
}

// Keep the no-preprocessor path available to plain Crust projects and editors.
pub(crate) fn required(
    entry: &Path,
    read: &mut impl FnMut(&Path) -> std::io::Result<String>,
) -> bool {
    let mut visited = HashSet::new();
    let mut pending = vec![entry.to_owned()];
    while let Some(path) = pending.pop() {
        let path = crate::source::identity(&path).unwrap_or(path);
        if !visited.insert(path.clone()) || visited.len() > 128 {
            continue;
        }
        let Ok(text) = read(&path) else {
            continue;
        };
        if needed(&text) {
            return true;
        }
        for import in imports(&text) {
            if let Some(name) = import.literal {
                if import.library.is_none()
                    && Path::new(&name).components().any(|part| {
                        matches!(
                            part,
                            std::path::Component::Prefix(_) | std::path::Component::RootDir
                        )
                    })
                {
                    continue;
                }
                pending.push(path.parent().unwrap().join(name));
            }
        }
    }
    return false;
}

impl Snapshot {
    pub fn create(
        entry: &Path,
        text: &str,
        native: &crate::native::Options,
        read: &mut impl FnMut(&Path) -> std::io::Result<String>,
    ) -> Result<Self, SourceError> {
        let temporary = std::env::temp_dir();
        let directory = loop {
            let path = temporary.join(format!(
                "crust-source-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => break path,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(error(entry, e.to_string())),
            }
        };
        let epoch = std::env::var_os("SOURCE_DATE_EPOCH").unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .to_string()
                .into()
        });
        let directory = directory
            .canonicalize()
            .map_err(|e| error(entry, e.to_string()))?;
        let mut snapshot = Self {
            directory,
            entry: entry.to_owned(),
            inputs: BTreeMap::new(),
            staged: BTreeMap::new(),
            root_input: String::new(),
            epoch,
        };
        // Copy source/include trees once. Relative includes and include_next keep
        // their ordinary directory ordering inside this private filesystem view.
        let mut roots = vec![entry.parent().unwrap().to_owned()];
        roots.extend(native.include_dirs.iter().cloned());
        let mut seen_directories = HashSet::new();
        while let Some(root) = roots.pop() {
            snapshot.copy_tree(&root, read, &mut seen_directories)?;
        }
        snapshot.inputs.insert(entry.to_owned(), text.to_owned());
        snapshot
            .staged
            .insert(entry.to_owned(), snapshot.path(entry));

        // Literal imports/includes can name files outside the initial trees.
        // Inactive missing imports are left for CC's conditional processing.
        let mut scanned = HashSet::new();
        loop {
            let files: Vec<_> = snapshot
                .inputs
                .iter()
                .filter(|(path, _)| !scanned.contains(*path))
                .map(|(p, t)| (p.clone(), t.clone()))
                .collect();
            if files.is_empty() {
                break;
            }
            for (path, text) in files {
                scanned.insert(path.clone());
                let mut names: Vec<_> = imports(&text)
                    .into_iter()
                    .filter_map(|i| if i.library.is_some() { None } else { i.literal })
                    .filter(|name| {
                        !Path::new(name).is_absolute()
                            && !Path::new(name).components().any(|part| {
                                matches!(
                                    part,
                                    std::path::Component::Prefix(_) | std::path::Component::RootDir
                                )
                            })
                    })
                    .collect();
                names.extend(
                    imports(&text)
                        .into_iter()
                        .filter(|import| import.library.is_some())
                        .filter_map(|import| import.literal),
                );
                for (line, directive) in directive_lines(&text) {
                    if directive.is_none() {
                        continue;
                    }
                    let Some(rest) = line
                        .trim_start()
                        .strip_prefix('#')
                        .map(str::trim_start)
                        .and_then(|s| s.strip_prefix("include"))
                    else {
                        continue;
                    };
                    if let Some(name) = rest
                        .trim_start()
                        .strip_prefix('"')
                        .and_then(|s| s.split_once('"').map(|(name, _)| name))
                    {
                        names.push(name.to_owned());
                    }
                }
                // Old explicit native declarations still refer to their headers.
                let tokens = lexer::discovery_tokens(&text);
                for window in tokens.windows(3) {
                    if window[0].kind == Token::Extern
                        && matches!(&window[1].kind,Token::Identifier(name) if name=="header")
                        && let Token::StringLiteral(bytes) = &window[2].kind
                        && let Ok(name) = std::str::from_utf8(bytes)
                    {
                        names.push(name.to_owned());
                    }
                }
                for name in names {
                    let dependency = path.parent().unwrap().join(name);
                    if let Ok(dependency) = crate::source::identity(&dependency)
                        && !snapshot.inputs.contains_key(&dependency)
                        && let Ok(content) = read(&dependency)
                    {
                        snapshot.inputs.insert(dependency.clone(), content);
                        snapshot
                            .staged
                            .insert(dependency.clone(), snapshot.path(&dependency));
                        snapshot.copy_tree(
                            dependency.parent().unwrap(),
                            read,
                            &mut seen_directories,
                        )?;
                    }
                }
            }
        }
        let files: Vec<_> = snapshot
            .inputs
            .iter()
            .map(|(p, t)| (p.clone(), t.clone()))
            .collect();
        for (index, (path, text)) in files.into_iter().enumerate() {
            let staged = snapshot.staged[&path].clone();
            if let Some(parent) = staged.parent() {
                fs::create_dir_all(parent).map_err(|e| error(&path, e.to_string()))?;
            }
            let source = path.extension().is_some_and(|e| e == "cr");
            let body = if source {
                snapshot.source(&path, &text, index)?
            } else {
                format!(
                    "#line 1 {}\n{text}",
                    serde_json::to_string(&crate::source::display_path(&path)).unwrap()
                )
            };
            let body = snapshot.absolute_includes(&body);
            fs::write(&staged, &body).map_err(|e| error(&path, e.to_string()))?;
            if let Ok(modified) = fs::metadata(&path).and_then(|m| m.modified()) {
                let _ = fs::OpenOptions::new()
                    .write(true)
                    .open(&staged)
                    .and_then(|file| file.set_modified(modified));
            }
            if path == entry {
                snapshot.root_input = body;
            }
        }
        return Ok(snapshot);
    }

    fn path(&self, original: &Path) -> PathBuf {
        let display = crate::source::display_path(original)
            .replace('\\', "/")
            .replace(':', "_");
        return self
            .directory
            .join("files")
            .join(display.trim_start_matches('/'));
    }

    pub fn mapped(&self, path: &Path) -> PathBuf {
        let path = crate::source::identity(path).unwrap_or_else(|_| path.to_owned());
        return self
            .staged
            .get(&path)
            .cloned()
            .unwrap_or_else(|| self.path(&path));
    }

    fn copy_tree(
        &mut self,
        root: &Path,
        read: &mut impl FnMut(&Path) -> std::io::Result<String>,
        seen: &mut HashSet<PathBuf>,
    ) -> Result<(), SourceError> {
        let root = crate::source::identity(root).unwrap_or_else(|_| root.to_owned());
        if !seen.insert(root.clone()) {
            return Ok(());
        }
        let Ok(entries) = fs::read_dir(&root) else {
            return Ok(());
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if !matches!(
                    entry.file_name().to_str(),
                    Some(".git" | "target" | "node_modules" | "bin" | "obj" | ".codex" | ".agents")
                ) {
                    self.copy_tree(&path, read, seen)?;
                }
            } else if path.is_file()
                && matches!(
                    path.extension().and_then(|s| s.to_str()),
                    Some("cr" | "h" | "hpp" | "inc" | "inl" | "def")
                )
            {
                let path = crate::source::identity(&path).unwrap_or(path);
                if self.inputs.contains_key(&path) {
                    continue;
                }
                if self.inputs.len() >= 8192 {
                    return Err(error(&root, "source snapshot exceeds the file limit"));
                }
                if let Ok(text) = read(&path) {
                    self.staged.insert(path.clone(), self.path(&path));
                    self.inputs.insert(path, text);
                }
            }
        }
        return Ok(());
    }

    fn source(&self, path: &Path, text: &str, index: usize) -> Result<String, SourceError> {
        let mut body = String::new();
        let mut previous = 0;
        for import in imports(text) {
            if let Some(name) = &import.library
                && import.literal.is_none()
            {
                let message = crate::source::library_path(name).err().unwrap_or_else(|| {
                    format!(
                        "library import <{name}> changed during preprocessing; retry the operation"
                    )
                });
                return Err(SourceError {
                    path: path.to_owned(),
                    diagnostic: Diagnostic::new(import.span, message),
                });
            }
            if let Some(name) = &import.literal
                && import.library.is_none()
            {
                let name = Path::new(name);
                if name.is_absolute()
                    || name.components().any(|c| {
                        matches!(
                            c,
                            std::path::Component::Prefix(_) | std::path::Component::RootDir
                        )
                    })
                {
                    body.push_str(&text[previous..import.span.start]);
                    body.push_str("\n#error import paths must be relative to the source file\n");
                    previous = import.span.end;
                    continue;
                }
            }
            body.push_str(&text[previous..import.span.start]);
            let argument = if let Some(name) = import.literal {
                let dependency = path.parent().unwrap().join(name);
                let dependency = crate::source::identity(&dependency).unwrap_or(dependency);
                serde_json::to_string(&crate::source::display_path(&self.mapped(&dependency)))
                    .unwrap()
            } else {
                text[import.argument.clone()].trim().to_owned()
            };
            body.push_str(&format!(
                "\n#pragma crust import_begin\n#include {argument}\n#line {} {}\n",
                import.span.line,
                serde_json::to_string(&crate::source::display_path(path)).unwrap()
            ));
            previous = import.span.end;
        }
        body.push_str(&text[previous..]);
        return Ok(format!(
            "#ifndef __CRUST_SOURCE_{index}\n#define __CRUST_SOURCE_{index}\n#line 1 {}\n{body}\n#pragma crust source_end\n#endif\n",
            serde_json::to_string(&crate::source::display_path(path)).unwrap()
        ));
    }

    fn absolute_includes(&self, text: &str) -> String {
        let mut output = String::new();
        for (line, directive) in directive_lines(text) {
            let mut rewritten = false;
            if directive.is_some()
                && let Some(rest) = line
                    .trim_start()
                    .strip_prefix('#')
                    .map(str::trim_start)
                    .and_then(|s| s.strip_prefix("include"))
                && let Some((name, _)) = rest
                    .trim_start()
                    .strip_prefix('"')
                    .and_then(|s| s.split_once('"'))
                && Path::new(name).is_absolute()
                && let Ok(original) = crate::source::identity(Path::new(name))
                && let Some(staged) = self.staged.get(&original)
            {
                output.push_str(&format!(
                    "#include {}\n",
                    serde_json::to_string(&crate::source::display_path(staged)).unwrap()
                ));
                rewritten = true;
            }
            if !rewritten {
                output.push_str(line);
            }
        }
        return output;
    }

    pub fn generated(&self, text: &str) -> String {
        let mut output = text.to_owned();
        for (original, staged) in &self.staged {
            let original = crate::source::display_path(original);
            let old = format!("#include \"{}\"", original.replace('\\', "/"));
            let new = format!(
                "#include {}",
                serde_json::to_string(&crate::source::display_path(staged)).unwrap()
            );
            output = output.replace(&old, &new);
        }
        return output;
    }

    pub fn original(&self, path: &Path) -> PathBuf {
        return self
            .staged
            .iter()
            .find(|(_, staged)| staged.as_path() == path)
            .map(|(original, _)| original.clone())
            .unwrap_or_else(|| path.to_owned());
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
