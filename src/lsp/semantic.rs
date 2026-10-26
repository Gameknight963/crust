use super::*;
use crate::analysis::{Analysis, Location, SymbolKind};

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
    fn failed(message: impl Into<String>) -> Self {
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
struct DocumentParams {
    text_document: DocumentId,
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
    pub(super) analysis: Analysis,
    pub(super) file: usize,
    uris: HashMap<PathBuf, String>,
    uri: String,
}

impl Context {
    pub(super) fn load(uri: &str, documents: &HashMap<String, Document>) -> Option<Self> {
        let document = documents.get(uri)?;
        let Some(path) = uri_path(uri) else {
            return Some(Self {
                analysis: crate::analyze(&document.text).ok()?,
                file: 0,
                uris: HashMap::new(),
                uri: uri.to_owned(),
            });
        };
        let sources = load_sources(documents);
        for graph in sources.graphs {
            if let Some(file) = graph.files().iter().position(|file| file.path == path)
                && let Ok(analysis) = graph.analyze()
            {
                return Some(Self {
                    analysis,
                    file,
                    uris: sources.paths,
                    uri: uri.to_owned(),
                });
            }
        }
        return None;
    }

    fn range(&self, location: Location) -> Value {
        let text = &self.analysis.files[location.file].text;
        return json!({ "start": position(text, location.span.start), "end": position(text, location.span.end) });
    }

    fn uri(&self, file: usize) -> String {
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
    ) -> Result<Value, RequestError> {
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

    fn document_symbols(&self) -> Value {
        return Value::Array(self.analysis.symbols.iter().filter(|symbol| symbol.location.file == self.file && !matches!(symbol.kind, SymbolKind::Variable | SymbolKind::Parameter | SymbolKind::TypeParameter))
            .map(|symbol| {
                let kind = match symbol.kind {
                    SymbolKind::Function => 12, SymbolKind::Method => 6,
                    SymbolKind::Struct | SymbolKind::Union => 23, SymbolKind::Enum => 10,
                    SymbolKind::EnumMember => 22, SymbolKind::Field => 8,
                    SymbolKind::Interface => 11, SymbolKind::Alias => 26,
                    _ => 13,
                };
                let mut value = json!({ "name": symbol.name, "kind": kind, "location": self.location(symbol.location) });
                if let Some(container) = &symbol.container { value["containerName"] = json!(container); }
                return value;
            }).collect());
    }
}

pub(super) fn request(
    method: &str,
    params: Value,
    documents: &HashMap<String, Document>,
    document_changes: bool,
) -> Result<Value, RequestError> {
    if method == "textDocument/completion" {
        let params: PositionParams = serde_json::from_value(params)?;
        return Ok(super::completion::complete(params, documents));
    }
    if method == "textDocument/documentSymbol" {
        let params: DocumentParams = serde_json::from_value(params)?;
        return Ok(Context::load(&params.text_document.uri, documents)
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
    let Some(context) = Context::load(&params.text_document.uri, documents) else {
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
        return Ok(context.hover(offset));
    }
    if method == "textDocument/prepareRename" {
        return Ok(context.analysis.rename_target(context.file, offset)
            .map(|(symbol, location)| json!({ "range": context.range(location), "placeholder": symbol.name }))
            .unwrap_or(Value::Null));
    }
    if let Some(name) = new_name {
        return context.rename(offset, &name, documents, document_changes);
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
