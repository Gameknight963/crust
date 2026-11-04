use super::*;
use crate::analysis::{Analysis, Location, Symbol, SymbolKind};

pub(super) struct RequestError {
    pub code: i32,
    pub message: String,
}

impl From<serde_json::Error> for RequestError {
    fn from(error: serde_json::Error) -> Self {
        return Self {
            code: ErrorCode::InvalidParams as i32,
            message: error.to_string(),
        };
    }
}

impl RequestError {
    pub(super) fn failed(message: impl Into<String>) -> Self {
        return Self {
            code: -32803,
            message: message.into(),
        };
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenameParams {
    #[serde(flatten)]
    position: PositionParams,
    new_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DocumentParams {
    pub(super) text_document: DocumentId,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PositionParams {
    pub(super) text_document: DocumentId,
    pub(super) position: Position,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReferenceParams {
    #[serde(flatten)]
    position: PositionParams,
    context: ReferenceContext,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReferenceContext {
    include_declaration: bool,
}

#[derive(Deserialize)]
pub(super) struct Position {
    line: usize,
    character: usize,
}

pub(super) struct Context {
    pub(super) analysis: std::rc::Rc<Analysis>,
    pub(super) file: usize,
    pub(super) uris: HashMap<PathBuf, String>,
    pub(super) uri: String,
}

impl Context {
    pub(super) fn load(
        uri: &str,
        documents: &HashMap<String, Document>,
        roots: &[PathBuf],
        database: &database::Database,
    ) -> Option<Self> {
        return Self::load_inner(uri, documents, roots, false, database);
    }

    pub(super) fn load_completion(
        uri: &str,
        documents: &HashMap<String, Document>,
        roots: &[PathBuf],
        database: &database::Database,
    ) -> Option<Self> {
        if let Some(context) = Self::load_inner(uri, documents, roots, true, database) {
            return Some(context);
        }
        let text = &documents.get(uri)?.text;
        let repaired = close_delimiters(text)?;
        let mut documents = documents.clone();
        documents.get_mut(uri)?.text = repaired;
        return Self::load_inner(uri, &documents, roots, true, database);
    }

    fn load_inner(
        uri: &str,
        documents: &HashMap<String, Document>,
        roots: &[PathBuf],
        completion: bool,
        database: &database::Database,
    ) -> Option<Self> {
        let document = documents.get(uri)?;
        let Some(path) = uri_path(uri) else {
            return Some(Self {
                analysis: std::rc::Rc::new(
                    if completion {
                        crate::analyze_completion(&document.text)
                    } else {
                        crate::analyze(&document.text)
                    }
                    .ok()?,
                ),
                file: 0,
                uris: HashMap::new(),
                uri: uri.to_owned(),
            });
        };
        let snapshot = database.snapshot(documents, roots);
        let sources = &snapshot.sources;
        for (index, graph) in sources.graphs.iter().enumerate() {
            if let Some(file) = graph.files().iter().position(|file| file.path == path)
                && let Ok(analysis) = snapshot.analyze(index, completion)
            {
                return Some(Self {
                    analysis,
                    file,
                    uris: sources.paths.clone(),
                    uri: uri.to_owned(),
                });
            }
        }
        return None;
    }

    pub(super) fn range(&self, location: Location) -> Value {
        let text = &self.analysis.files[location.file].text;
        return json!({ "start": position(text, location.span.start), "end": position(text, location.span.end) });
    }

    pub(super) fn uri(&self, file: usize) -> String {
        return self.analysis.files[file]
            .path
            .as_ref()
            .map(|path| {
                self.uris
                    .get(path)
                    .cloned()
                    .unwrap_or_else(|| path_uri(path))
            })
            .unwrap_or_else(|| self.uri.clone());
    }

    fn location(&self, location: Location) -> Value {
        return json!({ "uri": self.uri(location.file), "range": self.range(location) });
    }

    fn rename(
        &self,
        offset: usize,
        name: &str,
        documents: &HashMap<String, Document>,
        document_changes: bool,
        workspace_roots: &[PathBuf],
    ) -> Result<Value, RequestError> {
        let global = self
            .analysis
            .rename_target(self.file, offset)
            .is_some_and(|(symbol, _)| {
                matches!(
                    symbol.kind,
                    SymbolKind::Function
                        | SymbolKind::Method
                        | SymbolKind::Alias
                        | SymbolKind::Interface
                        | SymbolKind::TypeParameter
                        | SymbolKind::Enum
                        | SymbolKind::Struct
                        | SymbolKind::Union
                        | SymbolKind::Field
                        | SymbolKind::EnumMember
                        | SymbolKind::Global
                )
            });
        let changes = if global {
            super::rename::global_edits(self, offset, name, documents, workspace_roots)?
        } else {
            let edits = self
                .analysis
                .rename(self.file, offset, name)
                .map_err(RequestError::failed)?;
            let mut changes: std::collections::BTreeMap<String, Vec<Value>> =
                std::collections::BTreeMap::new();
            for edit in edits {
                changes
                    .entry(self.uri(edit.location.file))
                    .or_default()
                    .push(json!({ "range": self.range(edit.location), "newText": edit.new_text }));
            }
            changes
        };
        if document_changes {
            let changes: Vec<_> = changes.into_iter().map(|(uri, edits)| {
                let version = documents.get(&uri).map(|document| document.version);
                return json!({ "textDocument": { "uri": uri, "version": version }, "edits": edits });
            }).collect();
            return Ok(json!({ "documentChanges": changes }));
        }
        return Ok(json!({ "changes": changes }));
    }

    fn hover(&self, offset: usize) -> Value {
        let definitions = self.analysis.definitions_at(self.file, offset);
        if !definitions.is_empty() {
            let mut details = Vec::new();
            for symbol in &definitions {
                if !details.contains(&symbol.detail) {
                    details.push(symbol.detail.clone());
                }
            }
            let value = details
                .iter()
                .map(|detail| code_block(detail))
                .collect::<Vec<_>>()
                .join("\n\n");
            let selection = self
                .analysis
                .references
                .iter()
                .filter(|reference| {
                    reference.location.file == self.file
                        && reference.location.span.start <= offset
                        && offset <= reference.location.span.end
                })
                .min_by_key(|reference| reference.location.span.end - reference.location.span.start)
                .map(|reference| reference.location)
                .unwrap_or(definitions[0].location);
            return json!({ "contents": { "kind": "markdown", "value": value }, "range": self.range(selection) });
        }
        if let Some(info) = self.analysis.type_at(self.file, offset) {
            return json!({ "contents": { "kind": "markdown", "value": code_block(&info.description) }, "range": self.range(info.location) });
        }
        return Value::Null;
    }

    pub(super) fn symbol_information(&self, symbol: &Symbol, location: Location) -> Value {
        let kind = match symbol.kind {
            SymbolKind::Function => 12,
            SymbolKind::Method => 6,
            SymbolKind::Struct | SymbolKind::Union => 23,
            SymbolKind::Enum => 10,
            SymbolKind::EnumMember => 22,
            SymbolKind::Field => 8,
            SymbolKind::Interface => 11,
            SymbolKind::Alias => 26,
            _ => 13,
        };
        let mut value =
            json!({ "name": symbol.name, "kind": kind, "location": self.location(location) });
        if let Some(container) = &symbol.container {
            value["containerName"] = json!(container);
        }
        return value;
    }

    fn document_symbols(&self) -> Value {
        return Value::Array(
            self.analysis
                .symbols
                .iter()
                .filter(|symbol| {
                    symbol.location.file == self.file
                        && !matches!(
                            symbol.kind,
                            SymbolKind::Variable
                                | SymbolKind::Parameter
                                | SymbolKind::TypeParameter
                        )
                })
                .map(|symbol| self.symbol_information(symbol, symbol.location))
                .collect(),
        );
    }
}

// Append missing delimiters only to the editor snapshot. Original offsets and
// source text remain intact, and rename continues to require strict analysis.
fn close_delimiters(text: &str) -> Option<String> {
    use crate::lexer::Token;
    let tokens = crate::lexer::discovery_tokens(text);
    let mut stack = Vec::new();
    for token in &tokens {
        match token.kind {
            Token::LeftBrace => stack.push('}'),
            Token::LeftParen => stack.push(')'),
            Token::LeftBracket => stack.push(']'),
            Token::RightBrace | Token::RightParen | Token::RightBracket => {
                let expected = match token.kind {
                    Token::RightBrace => '}',
                    Token::RightParen => ')',
                    _ => ']',
                };
                if stack.pop() != Some(expected) {
                    return None;
                }
            }
            _ => {}
        }
    }
    if stack.is_empty() {
        return None;
    }
    let mut repaired = text.to_owned();
    repaired.push('\n');
    let mut terminated = tokens
        .iter()
        .rev()
        .find(|token| token.kind != Token::EndOfFile)
        .is_some_and(|token| {
            matches!(
                token.kind,
                Token::Semicolon | Token::RightBrace | Token::LeftBrace
            )
        });
    while let Some(closing) = stack.pop() {
        if closing == '}' && !terminated {
            repaired.push(';');
        }
        repaired.push(closing);
        terminated = closing == '}';
    }
    return Some(repaired);
}

pub(super) fn request(
    method: &str,
    params: Value,
    documents: &HashMap<String, Document>,
    document_changes: bool,
    workspace_roots: &[PathBuf],
    database: &database::Database,
    native_hover: bool,
) -> Result<Value, RequestError> {
    if method == "textDocument/signatureHelp" {
        return super::signature::request(params, documents, workspace_roots, database);
    }
    if method == "workspace/symbol" {
        return super::workspace::symbols(params, documents, workspace_roots, database);
    }
    if method == "textDocument/completion" {
        return super::completion::request(params, documents, workspace_roots, database);
    }
    if method == "textDocument/documentSymbol" {
        let params: DocumentParams = serde_json::from_value(params)?;
        return Ok(Context::load(
            &params.text_document.uri,
            documents,
            workspace_roots,
            database,
        )
        .or_else(|| {
            Context::load_completion(
                &params.text_document.uri,
                documents,
                workspace_roots,
                database,
            )
        })
        .map(|context| context.document_symbols())
        .unwrap_or(Value::Null));
    }
    let (params, include_declarations, new_name) = if method == "textDocument/references" {
        let params: ReferenceParams = serde_json::from_value(params)?;
        (
            params.position,
            Some(params.context.include_declaration),
            None,
        )
    } else if method == "textDocument/rename" {
        let params: RenameParams = serde_json::from_value(params)?;
        (params.position, None, Some(params.new_name))
    } else {
        let params: PositionParams = serde_json::from_value(params)?;
        (params, None, None)
    };
    if method == "textDocument/hover"
        && let Some(document) = documents.get(&params.text_document.uri)
        && let Some(cursor) = offset(&document.text, &params.position)
        && super::directives::contains(&document.text, cursor)
    {
        return Ok(Value::Null);
    }
    let context = Context::load(
        &params.text_document.uri,
        documents,
        workspace_roots,
        database,
    )
    .or_else(|| {
        if matches!(
            method,
            "textDocument/hover"
                | "textDocument/definition"
                | "textDocument/references"
                | "textDocument/documentHighlight"
        ) {
            return Context::load_completion(
                &params.text_document.uri,
                documents,
                workspace_roots,
                database,
            );
        }
        return None;
    });
    let Some(context) = context else {
        if new_name.is_some() {
            return Err(RequestError::failed(
                "rename requires a successfully checked program",
            ));
        }
        return Ok(Value::Null);
    };
    let Some(offset) = offset(&context.analysis.files[context.file].text, &params.position) else {
        return Ok(Value::Null);
    };
    if method == "textDocument/hover" {
        let mut hover = context.hover(offset);
        if native_hover {
            super::hover::classified(&context, &mut hover);
        }
        return Ok(hover);
    }
    if method == "textDocument/prepareRename" {
        return Ok(context.analysis.rename_target(context.file, offset)
            .map(|(symbol, location)| json!({ "range": context.range(location), "placeholder": symbol.name }))
            .unwrap_or(Value::Null));
    }
    if method == "textDocument/documentHighlight" {
        return Ok(Value::Array(
            context
                .analysis
                .references_at(context.file, offset, true)
                .into_iter()
                .filter(|location| location.file == context.file)
                .map(|location| json!({"range": context.range(location), "kind": 1}))
                .collect(),
        ));
    }
    if let Some(name) = new_name {
        return context.rename(offset, &name, documents, document_changes, workspace_roots);
    }
    if let Some(include_declarations) = include_declarations {
        return Ok(Value::Array(
            context
                .analysis
                .references_at(context.file, offset, include_declarations)
                .into_iter()
                .map(|location| context.location(location))
                .collect(),
        ));
    }
    let definitions = context.analysis.definitions_at(context.file, offset);
    return Ok(Value::Array(
        definitions
            .iter()
            .map(|symbol| context.location(symbol.location))
            .collect(),
    ));
}

pub(super) fn plain_hover(value: &mut Value) {
    if let Some(contents) = value.get_mut("contents")
        && let Some(text) = contents["value"].as_str()
    {
        let text = text
            .lines()
            .filter(|line| !line.starts_with("```"))
            .collect::<Vec<_>>()
            .join("\n");
        *contents = json!({"kind":"plaintext", "value":text});
    }
}

fn code_block(text: &str) -> String {
    let length = text
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat((length + 1).max(3));
    return format!("{fence}crust\n{text}\n{fence}");
}

pub(super) fn offset(text: &str, target: &Position) -> Option<usize> {
    let (mut line, mut character, mut offset) = (0, 0, 0);
    while offset < text.len() {
        if line == target.line && character == target.character {
            return Some(offset);
        }
        if line > target.line || (line == target.line && character > target.character) {
            return None;
        }
        let ch = text[offset..].chars().next().unwrap();
        offset += ch.len_utf8();
        match ch {
            '\r' => {
                if text.as_bytes().get(offset) == Some(&b'\n') {
                    offset += 1;
                }
                line += 1;
                character = 0;
            }
            '\n' => {
                line += 1;
                character = 0;
            }
            _ => character += ch.len_utf16(),
        }
    }
    return if line == target.line && character == target.character {
        Some(offset)
    } else {
        None
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_offsets_handle_unicode_crlf_cr_and_invalid_positions() {
        let text = "a🦀b\r\nx\ry\n";
        for expected in text
            .char_indices()
            .map(|(offset, _)| offset)
            .chain(std::iter::once(text.len()))
        {
            if text.as_bytes().get(expected) == Some(&b'\n')
                && expected > 0
                && text.as_bytes()[expected - 1] == b'\r'
            {
                continue;
            }
            let point = position(text, expected);
            let target: Position = serde_json::from_value(point).unwrap();
            assert_eq!(offset(text, &target), Some(expected));
        }
        assert_eq!(
            offset(
                text,
                &Position {
                    line: 0,
                    character: 2
                }
            ),
            None
        );
        assert_eq!(
            offset(
                text,
                &Position {
                    line: 20,
                    character: 0
                }
            ),
            None
        );
        assert_eq!(
            offset(
                text,
                &Position {
                    line: 0,
                    character: 99
                }
            ),
            None
        );
        assert!(code_block("a```b").starts_with("````crust\n"));
    }
}
