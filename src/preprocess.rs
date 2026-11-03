//! CC owns preprocessing. This module separates languages and maps expanded text
//! back to the source files used by diagnostics and editor analysis.
use crate::diagnostic::{Diagnostic, Span};
use crate::lexer::{self, SpannedToken, Token};
use crate::source::SourceError;
use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
mod snapshot;
pub(crate) use snapshot::required;
pub(crate) const PREPROCESSED: &str = "/* Crust preprocessed C. Compile with -x cpp-output. */\n";
const GENERATED: &str = "#pragma crust generated_begin";

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
    pub replay: Replay,
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

pub(crate) struct Replay {
    snapshot: snapshot::Snapshot,
    cc: std::ffi::OsString,
    native: crate::native::Options,
    shared: bool,
    pic: bool,
    expanded: String,
    frozen: String,
    external: BTreeMap<PathBuf, String>,
}

impl Replay {
    fn command(&self) -> Command {
        let mut command = Command::new(&self.cc);
        crate::native::configure_windows_toolchain(&mut command);
        command.args([
            "-E",
            "-dD",
            "-x",
            "c",
            "-std=c11",
            self.native.profile.compiler_flag(),
        ]);
        crate::native::instrumentation(&mut command, &self.native);
        if self.shared {
            command.arg("-DCRUST_BUILD_SHARED");
        }
        if !cfg!(windows) && self.pic {
            command.arg("-fPIC");
        }
        command.env("SOURCE_DATE_EPOCH", &self.snapshot.epoch);
        if let Some(parent) = self.snapshot.mapped(&self.snapshot.entry).parent() {
            command.current_dir(parent);
        }
        for directory in &self.native.include_dirs {
            command.arg("-I").arg(crate::source::display_path(
                &self.snapshot.mapped(directory),
            ));
        }
        for define in &self.native.defines {
            command.arg("-D").arg(define);
        }
        // The entry is fed through stdin, with its original filename in #line.
        command.arg("-iquote").arg(crate::source::display_path(
            &self.snapshot.mapped(self.snapshot.entry.parent().unwrap()),
        ));
        command.arg("-");
        return command;
    }

    fn validate(&self) -> Result<(), String> {
        for (path, expected) in &self.external {
            if !std::fs::read_to_string(path).is_ok_and(|text| text == *expected) {
                return Err(format!(
                    "C preprocessing input changed during compilation: {}",
                    crate::source::display_path(path)
                ));
            }
        }
        return Ok(());
    }

    pub fn finish(&self, generated: &str) -> Result<String, String> {
        self.validate()?;
        let generated = self.snapshot.generated(generated);
        let input = format!("{}\n{GENERATED}\n{generated}", self.snapshot.root_input);
        let result = output(self.command(), &input)?;
        if !result.status.success() {
            return Err(format!(
                "C preprocessing failed for generated code:\n{}",
                String::from_utf8_lossy(&result.stderr)
            ));
        }
        self.validate()?;
        let expanded =
            String::from_utf8(result.stdout).map_err(|_| "C preprocessor output is not UTF-8")?;
        let (prefix, generated) = expanded
            .split_once(GENERATED)
            .ok_or("C preprocessor omitted the generated-code boundary")?;
        if prefix.trim() != self.expanded.trim() {
            return Err("C preprocessing changed between checking and emission".to_owned());
        }
        return Ok(format!(
            "{PREPROCESSED}{}\n{}",
            self.frozen,
            without_definitions(generated)
        ));
    }
}

fn without_definitions(text: &str) -> String {
    let mut result = String::new();
    let mut continuation = false;
    for line in text.split_inclusive('\n') {
        let hidden = continuation || line.starts_with("#define ") || line.starts_with("#undef ");
        continuation = hidden && line.trim_end_matches(['\r', '\n']).ends_with('\\');
        if hidden {
            result.push('\n');
        } else {
            result.push_str(line);
        }
    }
    return result;
}

pub(crate) fn prepare(
    path: &Path,
    text: &str,
    native: &crate::native::Options,
    shared: bool,
    pic: bool,
    read: &mut impl FnMut(&Path) -> std::io::Result<String>,
) -> Result<Prepared, SourceError> {
    crate::native::validate_defines(&native.defines).map_err(|e| error(path, e))?;
    let snapshot = snapshot::Snapshot::create(path, text, native, read)?;
    let cc = std::env::var_os("CC").unwrap_or_else(|| {
        if cfg!(windows) {
            "clang".into()
        } else {
            "cc".into()
        }
    });
    let cc = if Path::new(&cc).components().count() > 1 {
        std::path::absolute(&cc)
            .map_err(|e| error(path, e.to_string()))?
            .into_os_string()
    } else {
        cc
    };
    let mut replay = Replay {
        snapshot,
        cc,
        native: native.clone(),
        shared,
        pic,
        expanded: String::new(),
        frozen: String::new(),
        external: BTreeMap::new(),
    };
    let output = output(replay.command(), &replay.snapshot.root_input)
        .map_err(|message| error(path, message))?;
    if !output.status.success() {
        return Err(compiler_error(
            path,
            text,
            read,
            format!(
                "C preprocessing failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }
    replay.expanded = String::from_utf8(output.stdout)
        .map_err(|_| error(path, "C preprocessor output is not UTF-8"))?;
    let expanded = without_definitions(&replay.expanded);
    let mut result = Prepared {
        text: String::new(),
        tokens: Vec::new(),
        origins: Vec::new(),
        inputs: BTreeMap::from([(path.to_owned(), text.to_owned())]),
        replay,
    };
    let mut filename = path.to_owned();
    let mut line_number = 1usize;
    let mut in_c = false;
    let mut native_expanded = String::new();
    let mut first_native = None;
    let mut depth = 0usize;

    for line in expanded.split_inclusive('\n') {
        if let Some((number, name)) = marker(line) {
            line_number = number;
            if !name.starts_with('<') {
                let physical = PathBuf::from(&name);
                let physical = if physical.is_absolute() {
                    physical
                } else {
                    result
                        .replay
                        .snapshot
                        .mapped(&result.replay.snapshot.entry)
                        .parent()
                        .unwrap()
                        .join(physical)
                };
                let physical = crate::source::identity(&physical).unwrap_or(physical);
                filename = result.replay.snapshot.original(&physical);
                let entered = line
                    .split_whitespace()
                    .rev()
                    .take_while(|flag| flag.parse::<u32>().is_ok())
                    .any(|flag| flag == "1");
                if entered && physical == filename && physical.is_file() {
                    let content = std::fs::read_to_string(&physical)
                        .map_err(|e| error(&physical, e.to_string()))?;
                    if result
                        .replay
                        .snapshot
                        .inputs
                        .get(&physical)
                        .is_some_and(|expected| *expected != content)
                    {
                        return Err(error(
                            &physical,
                            "save this included C header before preprocessing its dependents",
                        ));
                    }
                    result.replay.external.insert(physical.clone(), content);
                }
                if !result.inputs.contains_key(&filename) {
                    let content =
                        if let Some(content) = result.replay.snapshot.inputs.get(&filename) {
                            content.clone()
                        } else {
                            read(&filename).map_err(|e| error(&filename, e.to_string()))?
                        };
                    result.inputs.insert(filename.clone(), content);
                }
            }
            native_expanded.push_str(line);
            continue;
        }
        if line.trim() == "#pragma crust import_begin" {
            if depth != 0 {
                return Err(error(&filename, "imports must be top-level declarations"));
            }
            line_number += 1;
            continue;
        }
        if line.trim() == "#pragma crust source_end" {
            if in_c {
                return Err(error(
                    &filename,
                    "C region requires #pragma crust end_c before the source file ends",
                ));
            }
            let source = &result.inputs[&filename];
            let span = Span {
                start: source.len(),
                end: source.len(),
                line: physical_lines(source).len().max(1),
                column: 1,
            };
            let offset = result.text.len();
            result.origins.push(Origin {
                start: offset,
                end: offset,
                path: filename.clone(),
                span,
                generated: false,
            });
            result.tokens.push(SpannedToken {
                kind: Token::EndOfFile,
                span: Span {
                    start: offset,
                    end: offset,
                    ..span
                },
            });
            result.text.push('\n');
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
                    "# {} {}\n",
                    line_number + 1,
                    serde_json::to_string(&crate::source::display_path(&filename)).unwrap()
                ));
            }
            line_number += 1;
            continue;
        }
        if in_c || line.trim_start().starts_with('#') {
            native_expanded.push_str(line);
        } else {
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
    // Preprocessing follows include order. Parsing keeps each Crust file in one
    // group so file-local bindings and generic scopes retain their identity.
    let mut groups: Vec<(PathBuf, Vec<SpannedToken>)> = vec![(path.to_owned(), Vec::new())];
    for token in std::mem::take(&mut result.tokens) {
        let file = result
            .origins
            .partition_point(|origin| origin.start <= token.span.start)
            .checked_sub(1)
            .map(|index| result.origins[index].path.clone())
            .unwrap_or_else(|| path.to_owned());
        let index = groups
            .iter()
            .position(|(path, _)| *path == file)
            .unwrap_or_else(|| {
                groups.push((file, Vec::new()));
                groups.len() - 1
            });
        groups[index].1.push(token);
    }
    result.tokens = groups.into_iter().flat_map(|(_, tokens)| tokens).collect();
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
        let target = Command::new(&result.replay.cc)
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
        let native_code = native_expanded.clone();
        result.tokens.insert(
            0,
            SpannedToken {
                kind: Token::NativeCode(native_code),
                span,
            },
        );
    }
    result.replay.frozen = native_expanded;
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
