use super::*;

pub(super) fn register(connection: &Connection, params: &Value) -> ServerResult<()> {
    if params["capabilities"]["workspace"]["didChangeWatchedFiles"]["dynamicRegistration"] != true {
        return Ok(());
    }
    connection.sender.send(
        lsp_server::Request::new(
            "crust-register-file-watchers".to_owned().into(),
            "client/registerCapability".to_owned(),
            json!({ "registrations": [{
                "id": "crust-source-files",
                "method": "workspace/didChangeWatchedFiles",
                "registerOptions": { "watchers": [
                    { "globPattern": "**/*.cr", "kind": 7 },
                    { "globPattern": "**/crust.toml", "kind": 7 }
                ] }
            }] }),
        )
        .into(),
    )?;
    return Ok(());
}

#[derive(Deserialize)]
struct FileEvent {
    uri: String,
    #[serde(rename = "type")]
    kind: u8,
}

#[derive(Deserialize)]
struct Params {
    changes: Vec<FileEvent>,
}

pub(super) fn validate(params: Value) -> ServerResult<()> {
    let params: Params = serde_json::from_value(params)?;
    for change in params.changes {
        if !(1..=3).contains(&change.kind) {
            return Err("watched-file event type must be 1, 2, or 3".into());
        }
        if uri_path(&change.uri).is_none() {
            return Err("watched-file events require local file URIs".into());
        }
    }
    return Ok(());
}
