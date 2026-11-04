use super::semantic::RequestError;
use super::*;
use crate::lexer::Token;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Params {
    text_document: DocumentId,
}

/// Folding stays available while declarations or function bodies are incomplete.
pub(super) fn folding(
    params: Value,
    documents: &HashMap<String, Document>,
) -> Result<Value, RequestError> {
    let params: Params = serde_json::from_value(params)?;
    let Some(document) = documents.get(&params.text_document.uri) else {
        return Ok(json!([]));
    };
    let mut stack = Vec::new();
    let mut ranges = Vec::new();
    for token in crate::lexer::discovery_tokens(&document.text) {
        match token.kind {
            Token::LeftBrace => stack.push(token.span.line - 1),
            Token::RightBrace => {
                if let Some(start) = stack.pop() {
                    let end = token.span.line - 1;
                    if end > start {
                        ranges
                            .push(json!({ "startLine": start, "endLine": end, "kind": "region" }));
                    }
                }
            }
            _ => {}
        }
    }
    ranges.sort_by_key(|range| (range["startLine"].as_u64(), range["endLine"].as_u64()));
    return Ok(Value::Array(ranges));
}
