use super::*;
use crate::analysis::{Analysis, Location, SymbolKind};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocumentParams {
    text_document: DocumentId,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PositionParams {
    text_document: DocumentId,
    position: Position,
}

#[derive(Deserialize)]
struct Position {
    line: usize,
    character: usize,
}

struct Context {
    analysis: Analysis,
    file: usize,
    uris: HashMap<PathBuf, String>,
    uri: String,
}

impl Context {
    fn load(uri: &str, documents: &HashMap<String, Document>) -> Option<Self> {
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

    fn location(&self, location: Location) -> Value {
        let uri = self.analysis.files[location.file]
            .path
            .as_ref()
            .map(|path| {
                self.uris
                    .get(path)
                    .cloned()
                    .unwrap_or_else(|| path_uri(path))
            })
            .unwrap_or_else(|| self.uri.clone());
        return json!({ "uri": uri, "range": self.range(location) });
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
) -> Result<Value, String> {
    if method == "textDocument/documentSymbol" {
        let params: DocumentParams =
            serde_json::from_value(params).map_err(|error| error.to_string())?;
        return Ok(Context::load(&params.text_document.uri, documents)
            .map(|context| context.document_symbols())
            .unwrap_or(Value::Null));
    }
    let params: PositionParams =
        serde_json::from_value(params).map_err(|error| error.to_string())?;
    let Some(context) = Context::load(&params.text_document.uri, documents) else {
        return Ok(Value::Null);
    };
    let Some(offset) = offset(&context.analysis.files[context.file].text, &params.position) else {
        return Ok(Value::Null);
    };
    if method == "textDocument/hover" {
        return Ok(context.hover(offset));
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

fn offset(text: &str, target: &Position) -> Option<usize> {
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
