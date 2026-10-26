use lsp_server::{Connection, ErrorCode, Message, Notification, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::{Path, PathBuf};
mod semantic;

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
                "hoverProvider": true,
                "definitionProvider": true,
                "documentSymbolProvider": true,
                "textDocumentSync": { "openClose": true, "change": 1 }
            },
            "serverInfo": { "name": "crust-lsp", "version": env!("CARGO_PKG_VERSION") }
        }),
    )?;

    let mut documents = HashMap::new();
    let mut published = HashMap::new();
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
                } else if matches!(
                    request.method.as_str(),
                    "textDocument/hover"
                        | "textDocument/definition"
                        | "textDocument/documentSymbol"
                ) {
                    match semantic::request(&request.method, request.params, &documents) {
                        Ok(result) => Response::new_ok(request.id, result),
                        Err(message) => {
                            Response::new_err(request.id, ErrorCode::InvalidParams as i32, message)
                        }
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
                if let Err(error) =
                    document_notification(connection, &mut documents, &mut published, notification)
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
    published: &mut HashMap<String, Value>,
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
            documents.insert(item.uri.clone(), document);
            publish_all(connection, documents, published, &item.uri)?;
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
            publish_all(connection, documents, published, &params.text_document.uri)?;
        }
        "textDocument/didClose" => {
            let params: CloseParams = serde_json::from_value(notification.params)?;
            documents.remove(&params.text_document.uri);
            published.remove(&params.text_document.uri);
            connection.sender.send(
                Notification::new(
                    "textDocument/publishDiagnostics".into(),
                    json!({
                        "uri": params.text_document.uri, "diagnostics": []
                    }),
                )
                .into(),
            )?;
            publish_all(connection, documents, published, &params.text_document.uri)?;
        }
        _ => {}
    }
    return Ok(());
}

fn diagnostic_json(source: &str, diagnostic: crate::diagnostic::Diagnostic) -> Value {
    return json!({
        "range": {
            "start": position(source, diagnostic.span.start),
            "end": position(source, diagnostic.span.end)
        },
        "severity": 1,
        "source": "crust",
        "message": diagnostic.message
    });
}

struct Sources {
    paths: HashMap<PathBuf, String>,
    graphs: Vec<crate::source::SourceSet>,
    errors: Vec<crate::source::SourceError>,
}

fn source_text(
    path: &Path,
    paths: &HashMap<PathBuf, String>,
    documents: &HashMap<String, Document>,
) -> std::io::Result<String> {
    if let Some(uri) = paths.get(path) {
        return Ok(documents[uri].text.clone());
    }
    return std::fs::read_to_string(path);
}

fn load_sources(documents: &HashMap<String, Document>) -> Sources {
    let paths: HashMap<_, _> = documents
        .keys()
        .filter_map(|uri| uri_path(uri).map(|path| (path, uri.clone())))
        .collect();
    let read = |path: &Path| {
        return source_text(path, &paths, documents);
    };
    let mut graphs = Vec::new();
    let mut errors = Vec::new();
    for uri in documents.keys() {
        if let Some(path) = uri_path(uri) {
            match crate::source::SourceSet::load_with(&path, read) {
                Ok(sources) => graphs.push(sources),
                Err(error) => errors.push(error),
            }
        }
    }
    let manifests: HashSet<_> = paths
        .keys()
        .filter_map(|path| {
            path.parent()?
                .ancestors()
                .map(|directory| directory.join("crust.toml"))
                .find(|manifest| manifest.is_file())
        })
        .collect();
    for manifest in manifests {
        let Ok(project) = crate::project::Project::load(&manifest) else {
            continue;
        };
        for target in project.targets {
            if graphs
                .iter()
                .any(|sources| sources.files()[0].path == target.entry)
            {
                continue;
            }
            if let Ok(sources) = crate::source::SourceSet::load_with(&target.entry, read)
                && sources
                    .files()
                    .iter()
                    .any(|file| paths.contains_key(&file.path))
            {
                graphs.push(sources);
            }
        }
    }
    // Analyze entries before their open dependencies, including import cycles.
    graphs.sort_by(|a, b| {
        b.files()
            .len()
            .cmp(&a.files().len())
            .then_with(|| a.files()[0].path.cmp(&b.files()[0].path))
    });
    return Sources {
        paths,
        graphs,
        errors,
    };
}

fn publish_all(
    connection: &Connection,
    documents: &HashMap<String, Document>,
    published: &mut HashMap<String, Value>,
    active: &str,
) -> ServerResult<()> {
    let Sources {
        paths,
        graphs,
        errors,
    } = load_sources(documents);
    let mut diagnostics: HashMap<String, Vec<Value>> = documents
        .keys()
        .map(|uri| (uri.clone(), Vec::new()))
        .collect();
    for (uri, document) in documents {
        if uri_path(uri).is_none()
            && let Err(error) = crate::check(&document.text)
        {
            diagnostics
                .get_mut(uri)
                .unwrap()
                .push(diagnostic_json(&document.text, error));
        }
    }
    for error in errors {
        let target = paths
            .get(&error.path)
            .cloned()
            .unwrap_or_else(|| path_uri(&error.path));
        let text = source_text(&error.path, &paths, documents).unwrap_or_default();
        let values = diagnostics.entry(target).or_default();
        let value = diagnostic_json(&text, error.diagnostic);
        if !values.contains(&value) {
            values.push(value);
        }
    }
    let mut covered = HashSet::new();
    for sources in graphs {
        if covered.contains(&sources.files()[0].path) {
            continue;
        }
        covered.extend(sources.files().iter().map(|file| file.path.clone()));
        if let Err(error) = sources.check() {
            let target = paths
                .get(&error.path)
                .cloned()
                .unwrap_or_else(|| path_uri(&error.path));
            let text = &sources
                .files()
                .iter()
                .find(|file| file.path == error.path)
                .unwrap()
                .text;
            let values = diagnostics.entry(target).or_default();
            let value = diagnostic_json(text, error.diagnostic);
            if !values.contains(&value) {
                values.push(value);
            }
        }
    }
    for uri in published.keys() {
        diagnostics.entry(uri.clone()).or_default();
    }
    let mut uris: Vec<_> = diagnostics.keys().cloned().collect();
    uris.sort_by(|a, b| (a != active).cmp(&(b != active)).then_with(|| a.cmp(b)));
    for uri in uris {
        let mut payload = json!({ "uri": uri, "diagnostics": diagnostics[&uri] });
        if let Some(document) = documents.get(&uri) {
            payload["version"] = json!(document.version);
        }
        if published.get(&uri) != Some(&payload) {
            connection.sender.send(
                Notification::new("textDocument/publishDiagnostics".into(), payload.clone()).into(),
            )?;
            if !documents.contains_key(&uri) && payload["diagnostics"] == json!([]) {
                published.remove(&uri);
            } else {
                published.insert(uri, payload);
            }
        }
    }
    return Ok(());
}

fn uri_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let (host, encoded) = rest.split_once('/')?;
    let mut bytes = Vec::new();
    let mut index = 0;
    while index < encoded.len() {
        let byte = encoded.as_bytes()[index];
        if byte == b'%' {
            bytes.push(u8::from_str_radix(encoded.get(index + 1..index + 3)?, 16).ok()?);
            index += 3;
        } else {
            if matches!(byte, b'?' | b'#' | 0) {
                return None;
            }
            bytes.push(byte);
            index += 1;
        }
    }
    let path = String::from_utf8(bytes).ok()?;
    if path.contains('\0') {
        return None;
    }
    let local = host.is_empty() || host.eq_ignore_ascii_case("localhost");
    let path = if cfg!(windows) && !local {
        PathBuf::from(format!("//{host}/{path}"))
    } else if !local {
        return None;
    } else if cfg!(windows) && path.as_bytes().get(1) == Some(&b':') {
        PathBuf::from(path)
    } else {
        PathBuf::from(format!("/{path}"))
    };
    return crate::source::identity(&path).ok();
}

fn path_uri(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    let path = path
        .strip_prefix("//?/UNC/")
        .map(|path| format!("//{path}"))
        .unwrap_or_else(|| path.strip_prefix("//?/").unwrap_or(&path).to_owned());
    let mut uri = if path.starts_with("//") {
        "file:".to_owned()
    } else if path.starts_with('/') {
        "file://".to_owned()
    } else {
        "file:///".to_owned()
    };
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/:".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    return uri;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_round_trip_spaces_unicode_reserved_characters_and_existing_paths() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        for path in [root.to_owned(), root.join("target/uri test 🦀 #%.cr")] {
            let identity = crate::source::identity(&path).unwrap();
            assert_eq!(uri_path(&path_uri(&identity)), Some(identity));
        }
        for uri in [
            "untitled:main.cr",
            "file:///bad%",
            "file:///bad%GG",
            "file:///bad%00",
            "file:///bad#fragment",
        ] {
            assert!(uri_path(uri).is_none(), "{uri}");
        }
    }
}
