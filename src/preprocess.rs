//! CC owns preprocessing. This module separates languages and maps expanded text
//! back to the source files used by diagnostics and editor analysis.
use crate::diagnostic::{Diagnostic, Span};
use crate::lexer::{self, SpannedToken, Token};
use crate::source::SourceError;
use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub(crate) fn needed(text: &str) -> bool {
    return directive_lines(text)
        .iter()
        .any(|(_, directive)| *directive);
}

fn physical_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' || (*byte == b'\r' && bytes.get(index + 1) != Some(&b'\n')) {
            lines.push(&text[start..=index]);
            start = index + 1;
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    return lines;
}

fn directive_lines(text: &str) -> Vec<(&str, bool)> {
    let mut output = Vec::new();
    let mut block = false;
    let mut quote = None;
    let mut escape = false;
    for line in physical_lines(text) {
        let bytes = line.as_bytes();
        let mut i = 0;
        let mut content = false;
        let mut directive = false;
        while i < bytes.len() {
            let b = bytes[i];
            let next = bytes.get(i + 1).copied();
            if block {
                if b == b'*' && next == Some(b'/') {
                    block = false;
                    i += 2;
                } else {
                    i += 1;
                }
                continue;
            }
            if let Some(delimiter) = quote {
                if escape {
                    escape = false;
                } else if b == b'\\' {
                    escape = true;
                } else if b == delimiter {
                    quote = None;
                }
                i += 1;
                continue;
            }
            if b == b'/' && next == Some(b'*') {
                block = true;
                i += 2;
                continue;
            }
            if b == b'/' && next == Some(b'/') {
                break;
            }
            if !content && b == b'#' {
                directive = true;
            }
            if !b.is_ascii_whitespace() {
                content = true;
            }
            if b == b'"' || b == b'\'' {
                quote = Some(b);
            }
            i += 1;
        }
        output.push((line, directive));
    }
    return output;
}

pub(crate) fn editor_text(text: &str) -> std::borrow::Cow<'_, str> {
    if !needed(text) {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut result = String::with_capacity(text.len());
    let mut native = false;
    let mut continuation = false;
    for (line, directive) in directive_lines(text) {
        let selected = if directive { region(line) } else { None };
        let hidden = native || directive || continuation || selected.is_some();
        if let Some(value) = selected {
            native = value;
        }
        if hidden {
            result.extend(line.bytes().map(|b| {
                if matches!(b, b'\n' | b'\r') {
                    b as char
                } else {
                    ' '
                }
            }));
        } else {
            result.push_str(line);
        }
        continuation =
            (directive || continuation) && line.trim_end_matches(['\r', '\n']).ends_with('\\');
    }
    return std::borrow::Cow::Owned(result);
}

pub(crate) struct Origin {
    pub start: usize,
    pub end: usize,
    pub path: PathBuf,
    pub span: Span,
    pub generated: bool,
}
pub(crate) struct Prepared {
    pub text: String,
    pub tokens: Vec<SpannedToken>,
    pub origins: Vec<Origin>,
    pub inputs: BTreeMap<PathBuf, String>,
}

impl Prepared {
    pub fn locate(&self, span: Span) -> Option<(&Path, Span)> {
        let first = self
            .origins
            .partition_point(|origin| origin.start <= span.start)
            .checked_sub(1)?;
        let origin = &self.origins[first];
        let mut result = origin.span;
        let last = self
            .origins
            .partition_point(|origin| origin.start < span.end)
            .saturating_sub(1);
        if let Some(end) = self.origins.get(last)
            && end.path == origin.path
            && end.span.end >= result.start
        {
            result.end = end.span.end;
        }
        return Some((&origin.path, result));
    }
}

fn fallback() -> Span {
    return Span {
        start: 0,
        end: 0,
        line: 1,
        column: 1,
    };
}
fn error(path: &Path, message: impl Into<String>) -> SourceError {
    return SourceError {
        path: path.to_owned(),
        diagnostic: Diagnostic::new(fallback(), message),
    };
}

fn compiler_error(
    path: &Path,
    text: &str,
    read: &mut impl FnMut(&Path) -> std::io::Result<String>,
    message: String,
) -> SourceError {
    for line in message.lines() {
        let parts: Vec<_> = line.split(':').collect();
        for index in 1..parts.len().saturating_sub(1) {
            let (Ok(number), Ok(column)) = (
                parts[index].parse::<usize>(),
                parts[index + 1].parse::<usize>(),
            ) else {
                continue;
            };
            let filename = PathBuf::from(parts[..index].join(":"));
            let filename = crate::source::identity(&filename).unwrap_or(filename);
            let source = if filename == path {
                text.to_owned()
            } else {
                read(&filename).unwrap_or_default()
            };
            let start = line_start(&source, number);
            let content = source[start..].split(['\r', '\n']).next().unwrap_or("");
            let byte = content
                .char_indices()
                .nth(column.saturating_sub(1))
                .map_or(content.len(), |(offset, _)| offset);
            return SourceError {
                path: filename,
                diagnostic: Diagnostic::new(
                    Span {
                        start: start + byte,
                        end: start + byte,
                        line: number,
                        column,
                    },
                    message,
                ),
            };
        }
    }
    return error(path, message);
}

pub(crate) fn output(mut command: Command, input: &str) -> Result<std::process::Output, String> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take().unwrap();
    return std::thread::scope(|scope| {
        let writer = scope.spawn(move || stdin.write_all(input.as_bytes()));
        let result = child.wait_with_output().map_err(|e| e.to_string())?;
        let written = writer
            .join()
            .map_err(|_| "preprocessor input writer failed")?;
        if result.status.success() {
            written.map_err(|e| e.to_string())?;
        }
        return Ok(result);
    });
}

fn marker(line: &str) -> Option<(usize, String)> {
    let line = line
        .strip_prefix("# ")
        .or_else(|| line.strip_prefix("#line "))?;
    let (number, rest) = line.split_once(' ')?;
    let number = number.parse().ok()?;
    let rest = rest.trim_start();
    let mut escaped = false;
    for (index, ch) in rest.char_indices().skip(1) {
        if ch == '"' && !escaped {
            let tokens = lexer::tokenize_raw(&rest[..=index]).ok()?;
            let Token::StringLiteral(bytes) = &tokens.first()?.kind else {
                return None;
            };
            return Some((number, String::from_utf8(bytes.clone()).ok()?));
        }
        escaped = ch == '\\' && !escaped;
    }
    return None;
}

fn region(line: &str) -> Option<bool> {
    let line = line.split_once("//").map_or(line, |(line, _)| line);
    let line = line.split_once("/*").map_or(line, |(line, _)| line);
    let words: Vec<_> = line
        .trim_start()
        .strip_prefix('#')?
        .split_whitespace()
        .collect();
    if words == ["pragma", "crust", "begin_c"] {
        return Some(true);
    }
    if words == ["pragma", "crust", "end_c"] {
        return Some(false);
    }
    return None;
}

// Preserve the original native text and directives for final C compilation.
// Only language-selection pragmas are removed. Native macros are expanded by CC.
fn native_text(
    path: &Path,
    text: &str,
    includes: &BTreeMap<(PathBuf, usize), Option<String>>,
) -> String {
    let mut result = format!(
        "#line 1 {}\n",
        serde_json::to_string(&crate::source::display_path(path)).unwrap()
    );
    let mut native = false;
    let mut continuation = false;
    let mut replaced_include = false;
    for (index, (line, directive)) in directive_lines(text).into_iter().enumerate() {
        if let Some(value) = if directive { region(line) } else { None } {
            native = value;
            result.push('\n');
            continue;
        }
        let directive = directive || continuation;
        if replaced_include && continuation {
            result.push('\n');
        } else if let Some(header) = includes.get(&(path.to_owned(), index + 1)) {
            if let Some(header) = header {
                result.push_str(header);
            } else {
                // CC skipped a guarded or once-only include. Its contents are
                // already supplied by an earlier native include.
                result.push('\n');
            }
            replaced_include = true;
        } else if native || directive {
            result.push_str(line);
        } else {
            result.push('\n');
        }
        continuation = directive && line.trim_end_matches(['\r', '\n']).ends_with('\\');
        if !continuation {
            replaced_include = false;
        }
    }
    return result;
}

pub(crate) fn prepare(
    path: &Path,
    text: &str,
    native: &crate::native::Options,
    read: &mut impl FnMut(&Path) -> std::io::Result<String>,
) -> Result<Prepared, SourceError> {
    crate::native::validate_defines(&native.defines).map_err(|e| error(path, e))?;
    let cc = std::env::var_os("CC").unwrap_or_else(|| {
        if cfg!(windows) {
            "clang".into()
        } else {
            "cc".into()
        }
    });
    let mut command = Command::new(&cc);
    crate::native::configure_windows_toolchain(&mut command);
    command.args([
        "-E",
        "-dI",
        "-x",
        "c",
        "-std=c11",
        native.profile.compiler_flag(),
    ]);
    if let Some(parent) = path.parent() {
        command
            .arg("-iquote")
            .arg(crate::source::display_path(parent));
    }
    for directory in &native.include_dirs {
        command
            .arg("-I")
            .arg(crate::source::display_path(directory));
    }
    for define in &native.defines {
        command.arg("-D").arg(define);
    }
    command.arg("-");
    let input = format!(
        "#line 1 {}\n{text}",
        serde_json::to_string(&crate::source::display_path(path)).unwrap()
    );
    let result = output(command, &input).map_err(|e| {
        error(
            path,
            format!(
                "could not run C preprocessor '{}': {e}",
                cc.to_string_lossy()
            ),
        )
    })?;
    if !result.status.success() {
        return Err(compiler_error(
            path,
            text,
            read,
            format!(
                "C preprocessing failed:\n{}",
                String::from_utf8_lossy(&result.stderr)
            ),
        ));
    }
    let expanded = String::from_utf8(result.stdout)
        .map_err(|_| error(path, "C preprocessor output is not UTF-8"))?;
    let mut result = Prepared {
        text: String::new(),
        tokens: Vec::new(),
        origins: Vec::new(),
        inputs: BTreeMap::from([(path.to_owned(), text.to_owned())]),
    };
    let mut filename = path.to_owned();
    let mut line_number = 1usize;
    let mut in_c = false;
    let mut native_expanded = String::new();
    let mut first_native = None;
    let mut depth = 0usize;
    let mut includes = BTreeMap::new();
    let mut pending_include: Option<((PathBuf, usize), Option<String>)> = None;
    for line in expanded.split_inclusive('\n') {
        if let Some((number, name)) = marker(line) {
            line_number = number;
            if !name.starts_with('<') {
                filename = crate::source::identity(Path::new(&name))
                    .unwrap_or_else(|_| PathBuf::from(name));
                if !result.inputs.contains_key(&filename) {
                    let content = read(&filename).map_err(|e| error(&filename, e.to_string()))?;
                    if std::fs::read_to_string(&filename).is_ok_and(|disk| disk != content) {
                        return Err(error(
                            &filename,
                            "save this included C header before preprocessing its dependents",
                        ));
                    }
                    result.inputs.insert(filename.clone(), content);
                }
                if line
                    .split_whitespace()
                    .rev()
                    .take_while(|flag| flag.parse::<u32>().is_ok())
                    .any(|flag| flag == "1")
                    && let Some((origin, Some(spelling))) = pending_include.take()
                {
                    let local = origin
                        .0
                        .parent()
                        .and_then(|parent| crate::source::identity(&parent.join(&spelling)).ok())
                        .is_some_and(|local| local == filename);
                    let directive = if local {
                        format!(
                            "#include {}\n",
                            serde_json::to_string(&crate::source::display_path(&filename)).unwrap()
                        )
                    } else {
                        // A quoted include found on the configured search path
                        // must keep its position there for #include_next.
                        format!("#include <{spelling}>\n")
                    };
                    includes.insert(origin, Some(directive));
                }
            }
            if in_c {
                native_expanded.push_str(line);
            }
            continue;
        }
        if line.trim_start().starts_with("#include ")
            || line.trim_start().starts_with("#include_next ")
        {
            // Keep angle includes on CC's search path. Resolving a system
            // header to an absolute include would change #include_next.
            let quoted = line
                .trim_start()
                .split_once(' ')
                .and_then(|(_, name)| name.trim_start().strip_prefix('"'))
                .and_then(|name| name.split_once('"'))
                .map(|(name, _)| name.to_owned());
            let origin = (filename.clone(), line_number);
            if quoted.is_some() {
                includes.insert(origin.clone(), None);
            }
            pending_include = Some((origin, quoted));
            line_number += 1;
            continue;
        }
        if let Some(begin) = region(line) {
            if begin == in_c {
                return Err(error(
                    &filename,
                    "C regions cannot nest and end_c requires begin_c",
                ));
            }
            if begin && depth != 0 {
                return Err(error(&filename, "C regions currently require file scope"));
            }
            in_c = begin;
            if begin {
                first_native.get_or_insert((filename.clone(), line_number));
                native_expanded.push_str(&format!(
                    "#line {} {}\n",
                    line_number + 1,
                    serde_json::to_string(&crate::source::display_path(&filename)).unwrap()
                ));
            }
            line_number += 1;
            continue;
        }
        if in_c {
            native_expanded.push_str(line);
        } else if !line.trim_start().starts_with('#') {
            let source = &result.inputs[&filename];
            let original_start = line_start(source, line_number);
            let original = source[original_start..]
                .split(['\r', '\n'])
                .next()
                .unwrap_or("");
            let tokens = lexer::tokenize_raw(line).map_err(|mut diagnostic| {
                diagnostic.span.line = line_number;
                SourceError {
                    path: filename.clone(),
                    diagnostic,
                }
            })?;
            let original_tokens = lexer::tokenize_raw(original).unwrap_or_default();
            let matches = align(&tokens, &original_tokens);
            let base = result.text.len();
            result.text.push_str(line);
            for (index, mut token) in tokens
                .into_iter()
                .enumerate()
                .filter(|(_, token)| token.kind != Token::EndOfFile)
            {
                let (span, generated) =
                    if let Some(candidate) = matches[index].map(|index| &original_tokens[index]) {
                        (
                            Span {
                                start: original_start + candidate.span.start,
                                end: original_start + candidate.span.end,
                                line: line_number,
                                column: candidate.span.column,
                            },
                            false,
                        )
                    } else {
                        let before = matches[..index]
                            .iter()
                            .flatten()
                            .last()
                            .map_or(0, |index| index + 1);
                        let after = matches[index + 1..]
                            .iter()
                            .flatten()
                            .next()
                            .copied()
                            .unwrap_or(original_tokens.len());
                        let unmatched = original_tokens.get(before..after).unwrap_or(&[]);
                        let candidate = unmatched
                            .iter()
                            .find(|token| matches!(token.kind, Token::Identifier(_)))
                            .or_else(|| unmatched.first());
                        let start = candidate.map_or(0, |token| token.span.start);
                        let end = candidate.map_or(original.len(), |token| token.span.end);
                        (
                            Span {
                                start: original_start + start,
                                end: original_start + end,
                                line: line_number,
                                column: original[..start].chars().count() + 1,
                            },
                            true,
                        )
                    };
                token.span.start += base;
                token.span.end += base;
                token.span.line = span.line;
                token.span.column = span.column;
                match token.kind {
                    Token::LeftBrace => depth += 1,
                    Token::RightBrace => depth = depth.saturating_sub(1),
                    _ => {}
                }
                result.origins.push(Origin {
                    start: token.span.start,
                    end: token.span.end,
                    path: filename.clone(),
                    span,
                    generated,
                });
                result.tokens.push(token);
            }
        }
        line_number += 1;
    }
    if in_c {
        return Err(error(path, "C region requires #pragma crust end_c"));
    }
    if let Some((origin, number)) = first_native {
        let names: HashSet<_> = result
            .tokens
            .iter()
            .filter_map(|token| {
                if let Token::Identifier(name) = &token.kind {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect();
        let target = Command::new(&cc)
            .arg("-dumpmachine")
            .output()
            .ok()
            .filter(|r| r.status.success())
            .map(|r| String::from_utf8_lossy(&r.stdout).trim().to_owned());
        let declarations =
            crate::bindings::native_declarations(&native_expanded, target.as_deref(), &names)
                .map_err(|message| {
                    compiler_error(&origin, &result.inputs[&origin], read, message)
                })?;
        let base = result.text.len();
        result.text.push_str(&declarations.text);
        let span = Span {
            start: line_start(&result.inputs[&origin], number),
            end: line_start(&result.inputs[&origin], number),
            line: number,
            column: 1,
        };
        let mut declaration_tokens =
            lexer::tokenize_raw(&declarations.text).map_err(|e| error(&origin, e.to_string()))?;
        declaration_tokens.pop();
        for token in &mut declaration_tokens {
            token.span.start += base;
            token.span.end += base;
            token.span.line = number;
            token.span.column = 1;
            let (path, span) = if let Token::Identifier(name) = &token.kind {
                if let Some((path, line, _column)) = declarations.locations.get(name) {
                    let text = result.inputs.get(path).map(String::as_str).unwrap_or("");
                    let start = line_start(text, *line);
                    let text = text[start..].split(['\r', '\n']).next().unwrap_or("");
                    let column = text.find(name).unwrap_or(0);
                    (
                        path.clone(),
                        Span {
                            start: start + column,
                            end: start + column + name.len(),
                            line: *line,
                            column: text[..column].chars().count() + 1,
                        },
                    )
                } else {
                    (origin.clone(), span)
                }
            } else {
                (origin.clone(), span)
            };
            result.origins.push(Origin {
                start: token.span.start,
                end: token.span.end,
                path,
                span,
                generated: true,
            });
        }
        declaration_tokens.append(&mut result.tokens);
        result.tokens = declaration_tokens;
        let native_code = native_text(path, text, &includes);
        result.tokens.insert(
            0,
            SpannedToken {
                kind: Token::NativeCode(native_code),
                span,
            },
        );
    }
    let span = Span {
        start: text.len(),
        end: text.len(),
        line: text.lines().count().max(1),
        column: 1,
    };
    result.origins.sort_by_key(|o| o.start);
    result.origins.push(Origin {
        start: result.text.len(),
        end: result.text.len(),
        path: path.to_owned(),
        span,
        generated: false,
    });
    result.tokens.push(SpannedToken {
        kind: Token::EndOfFile,
        span: Span {
            start: result.text.len(),
            end: result.text.len(),
            ..span
        },
    });
    return Ok(result);
}

fn align(expanded: &[SpannedToken], original: &[SpannedToken]) -> Vec<Option<usize>> {
    let mut result = vec![None; expanded.len()];
    if expanded.len().saturating_mul(original.len()) > 1_000_000 {
        return result;
    }
    let mut scores = vec![vec![0usize; original.len() + 1]; expanded.len() + 1];
    fn weight(token: &Token) -> usize {
        return match token {
            Token::Identifier(_) => 32,
            Token::IntegerLiteral(_) | Token::StringLiteral(_) => 16,
            Token::LeftParen | Token::RightParen | Token::Semicolon | Token::Comma => 1,
            _ => 4,
        };
    }
    for i in (0..expanded.len()).rev() {
        for j in (0..original.len()).rev() {
            scores[i][j] = scores[i + 1][j].max(scores[i][j + 1]);
            if expanded[i].kind == original[j].kind {
                scores[i][j] = scores[i][j].max(weight(&expanded[i].kind) + scores[i + 1][j + 1]);
            }
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < expanded.len() && j < original.len() {
        if expanded[i].kind == original[j].kind
            && scores[i][j] == weight(&expanded[i].kind) + scores[i + 1][j + 1]
        {
            result[i] = Some(j);
            i += 1;
            j += 1;
        } else if scores[i + 1][j] >= scores[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    return result;
}

fn line_start(text: &str, line: usize) -> usize {
    if line <= 1 {
        return 0;
    }
    return physical_lines(text)
        .iter()
        .take(line - 1)
        .map(|line| line.len())
        .sum();
}
