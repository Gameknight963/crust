use lsp_server::{Connection, ErrorCode, Message, Notification, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::error::Error;

type ServerResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenParams {
    text_document: OpenDocument,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenDocument {
    uri: String,
    language_id: String,
    version: i32,
    text: String,
}

struct Document {
    version: i32,
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChangeParams {
    text_document: VersionedDocument,
    content_changes: Vec<ContentChange>,
}

#[derive(Deserialize)]
struct VersionedDocument {
    uri: String,
    version: i32,
}

#[derive(Deserialize)]
struct ContentChange {
    text: String,
    range: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CloseParams {
    text_document: DocumentId,
}

#[derive(Deserialize)]
struct DocumentId {
    uri: String,
}

pub fn serve(connection: &Connection) -> ServerResult<()> {
    let (id, _) = connection.initialize_start()?;
    connection.initialize_finish(
        id,
        json!({
            "capabilities": {
                "positionEncoding": "utf-16",
                "textDocumentSync": { "openClose": true, "change": 1 }
            },
            "serverInfo": { "name": "crust-lsp", "version": env!("CARGO_PKG_VERSION") }
        }),
    )?;

    let mut documents = HashMap::new();
    let mut shutdown = false;
    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                let response = if shutdown {
                    Response::new_err(
                        request.id,
                        ErrorCode::InvalidRequest as i32,
                        String::from("server is shutting down"),
                    )
                } else if request.method == "shutdown" {
                    if request.params.is_null() {
                        shutdown = true;
                        Response::new_ok(request.id, Value::Null)
                    } else {
                        Response::new_err(
                            request.id,
                            ErrorCode::InvalidParams as i32,
                            String::from("shutdown takes no parameters"),
                        )
                    }
                } else {
                    Response::new_err(
                        request.id,
                        ErrorCode::MethodNotFound as i32,
                        format!("unsupported request '{}'", request.method),
                    )
                };
                connection.sender.send(response.into())?;
            }
            Message::Notification(notification) => {
                if notification.method == "exit" {
                    if shutdown {
                        return Ok(());
                    }
                    return Err("exit received before shutdown".into());
                }
                if shutdown {
                    continue;
                }
                if let Err(error) = document_notification(connection, &mut documents, notification)
                {
                    connection.sender.send(
                        Notification::new(
                            "window/logMessage".into(),
                            json!({
                                "type": 2, "message": error.to_string()
                            }),
                        )
                        .into(),
                    )?;
                }
            }
            Message::Response(_) => {}
        }
    }
    return Err("client disconnected before exit".into());
}

fn document_notification(
    connection: &Connection,
    documents: &mut HashMap<String, Document>,
    notification: Notification,
) -> ServerResult<()> {
    match notification.method.as_str() {
        "textDocument/didOpen" => {
            let params: OpenParams = serde_json::from_value(notification.params)?;
            let item = params.text_document;
            if item.language_id != "crust" {
                return Ok(());
            }
            let document = Document {
                version: item.version,
                text: item.text,
            };
            publish(connection, &item.uri, &document)?;
            documents.insert(item.uri, document);
        }
        "textDocument/didChange" => {
            let params: ChangeParams = serde_json::from_value(notification.params)?;
            let Some(document) = documents.get_mut(&params.text_document.uri) else {
                return Err("didChange requires an open document".into());
            };
            if params.text_document.version <= document.version {
                return Ok(());
            }
            if params
                .content_changes
                .iter()
                .any(|change| change.range.is_some())
            {
                return Err("crust-lsp requests full-document synchronization".into());
            }
            if params.content_changes.is_empty() {
                return Ok(());
            }
            for change in params.content_changes {
                document.text = change.text;
            }
            document.version = params.text_document.version;
            publish(connection, &params.text_document.uri, document)?;
        }
        "textDocument/didClose" => {
            let params: CloseParams = serde_json::from_value(notification.params)?;
            documents.remove(&params.text_document.uri);
            connection.sender.send(
                Notification::new(
                    "textDocument/publishDiagnostics".into(),
                    json!({
                        "uri": params.text_document.uri, "diagnostics": []
                    }),
                )
                .into(),
            )?;
        }
        _ => {}
    }
    return Ok(());
}

fn publish(connection: &Connection, uri: &str, document: &Document) -> ServerResult<()> {
    let diagnostics = match crate::check(&document.text) {
        Ok(()) => Vec::new(),
        Err(diagnostic) => vec![json!({
            "range": {
                "start": position(&document.text, diagnostic.span.start),
                "end": position(&document.text, diagnostic.span.end)
            },
            "severity": 1,
            "source": "crust",
            "message": diagnostic.message
        })],
    };
    connection.sender.send(
        Notification::new(
            "textDocument/publishDiagnostics".into(),
            json!({
                "uri": uri, "version": document.version, "diagnostics": diagnostics
            }),
        )
        .into(),
    )?;
    return Ok(());
}

// Compiler spans use byte offsets and character columns. LSP's default uses
// UTF-16 code units, including two units for characters outside the BMP.
fn position(source: &str, offset: usize) -> Value {
    let mut end = offset.min(source.len());
    while !source.is_char_boundary(end) {
        end -= 1;
    }
    let mut line = 0;
    let mut character = 0;
    let mut chars = source[..end].chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
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
    return json!({ "line": line, "character": character });
}
