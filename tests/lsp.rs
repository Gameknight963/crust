use lsp_server::{Message, Notification, Request, Response};
use serde_json::{Value, json};
use std::io::BufReader;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

struct Server {
    child: Child,
    input: ChildStdin,
    output: Receiver<Message>,
    next_id: i32,
}

impl Server {
    fn start() -> Server {
        return Self::start_with_capabilities(json!({}));
    }

    fn start_with_capabilities(capabilities: Value) -> Server {
        return Self::start_with_params(json!({"capabilities": capabilities}));
    }

    fn start_with_params(params: Value) -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_crust-lsp"))
            .arg("--stdio")
            .env("CC", "missing-crust-test-compiler")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let (sender, output) = mpsc::channel();
        std::thread::spawn(move || {
            while let Some(message) = Message::read(&mut stdout).unwrap() {
                if sender.send(message).is_err() {
                    break;
                }
            }
        });
        let mut server = Server {
            child,
            input,
            output,
            next_id: 1,
        };
        let initialized = server.request("initialize", params);
        let capabilities = &initialized.result.unwrap()["capabilities"];
        assert_eq!(capabilities["positionEncoding"], "utf-16");
        assert_eq!(capabilities["textDocumentSync"]["change"], 1);
        assert_eq!(capabilities["hoverProvider"], true);
        assert_eq!(capabilities["definitionProvider"], true);
        assert_eq!(capabilities["referencesProvider"], true);
        assert_eq!(capabilities["renameProvider"]["prepareProvider"], true);
        assert_eq!(capabilities["documentSymbolProvider"], true);
        assert_eq!(capabilities["workspaceSymbolProvider"], true);
        assert_eq!(capabilities["semanticTokensProvider"]["full"], true);
        assert_eq!(
            capabilities["semanticTokensProvider"]["legend"]["tokenModifiers"],
            json!(["declaration", "defaultLibrary"])
        );
        assert_eq!(
            capabilities["signatureHelpProvider"]["triggerCharacters"],
            json!(["(", ","])
        );
        assert_eq!(
            capabilities["workspace"]["workspaceFolders"]["supported"],
            true
        );
        assert_eq!(capabilities["completionProvider"]["resolveProvider"], false);
        assert_eq!(
            capabilities["completionProvider"]["triggerCharacters"],
            json!([".", ":", ">"])
        );
        server.notify("initialized", json!({}));
        return server;
    }

    fn receive(&self) -> Message {
        return self
            .output
            .recv_timeout(Duration::from_secs(10))
            .expect("server should respond promptly with a framed LSP message");
    }

    fn request(&mut self, method: &str, params: Value) -> Response {
        let id = self.next_id;
        self.next_id += 1;
        Message::Request(Request::new(id.into(), method.into(), params))
            .write(&mut self.input)
            .unwrap();
        let Message::Response(response) = self.receive() else {
            panic!("expected a response");
        };
        assert_eq!(response.id, id.into());
        return response;
    }

    fn notify(&mut self, method: &str, params: Value) {
        Message::Notification(Notification::new(method.into(), params))
            .write(&mut self.input)
            .unwrap();
    }

    fn open(&mut self, uri: &str, version: i32, text: &str) -> Value {
        self.notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": uri, "languageId": "crust", "version": version, "text": text
            }}),
        );
        return self.diagnostics();
    }

    fn change(&mut self, uri: &str, version: i32, text: &str) {
        self.notify("textDocument/didChange", json!({
            "textDocument": { "uri": uri, "version": version }, "contentChanges": [{"text": text}]
        }));
    }

    fn diagnostics(&self) -> Value {
        let Message::Notification(notification) = self.receive() else {
            panic!("expected diagnostics");
        };
        assert_eq!(notification.method, "textDocument/publishDiagnostics");
        return notification.params;
    }

    fn shutdown(&mut self) {
        let response = self.request("shutdown", Value::Null);
        assert!(response.error.is_none());
        // Response's Option<Value> deserializes a JSON null result as None.
        assert_eq!(response.result.unwrap_or(Value::Null), Value::Null);
    }

    fn exit(mut self, success: bool) {
        self.notify("exit", Value::Null);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert_eq!(status.success(), success);
                break;
            }
            assert!(Instant::now() < deadline, "language server should exit");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn stop(mut self) {
        self.shutdown();
        self.exit(true);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn publishes_unsaved_compiler_errors_and_clears_them_when_fixed_or_closed() {
    let mut server = Server::start();
    let uri = "file:///does-not-exist/main.cr";
    let diagnostic = server.open(uri, 1, "int main() { return missing; }");
    assert_eq!(diagnostic["uri"], uri);
    assert_eq!(diagnostic["version"], 1);
    assert!(
        diagnostic["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("unknown variable")
    );
    assert_eq!(diagnostic["diagnostics"][0]["severity"], 1);
    server.change(uri, 2, "int main() { return 0; }");
    let fixed = server.diagnostics();
    assert_eq!(fixed["version"], 2);
    assert_eq!(fixed["diagnostics"], json!([]));
    server.notify(
        "textDocument/didClose",
        json!({"textDocument": {"uri": uri}}),
    );
    assert_eq!(server.diagnostics()["diagnostics"], json!([]));
    server.stop();
}

#[test]
fn converts_unicode_byte_spans_to_utf16_and_handles_crlf_and_eof() {
    let mut server = Server::start();
    let uri = "untitled:unicode.cr";
    let text = "int main() { /* 🦀 café */ return missing; }";
    let response = server.open(uri, 1, text);
    let start = text[..text.find("missing").unwrap()].encode_utf16().count();
    assert_eq!(
        response["diagnostics"][0]["range"],
        json!({
            "start": {"line": 0, "character": start}, "end": {"line": 0, "character": start + 7}
        })
    );
    server.change(uri, 2, "// 🦀\r\nint main() {\r\n return missing;\r\n}");
    assert_eq!(
        server.diagnostics()["diagnostics"][0]["range"]["start"],
        json!({"line": 2, "character": 8})
    );
    server.change(uri, 3, "int main() {\r\n");
    assert_eq!(
        server.diagnostics()["diagnostics"][0]["range"]["start"],
        json!({"line": 1, "character": 0})
    );
    server.stop();
}

#[test]
fn ignores_stale_versions_and_keeps_documents_independent() {
    let mut server = Server::start();
    server.open("untitled:a.cr", 10, "int main() { return missing; }");
    server.open("untitled:b.cr", 1, "int main() { return 0; }");
    server.change("untitled:a.cr", 9, "int main() { return 0; }");
    let barrier = server.request("test/barrier", Value::Null);
    assert_eq!(barrier.error.unwrap().code, -32601);
    server.change("untitled:a.cr", 11, "int main() { return missing; }");
    let response = server.diagnostics();
    assert_eq!(response["uri"], "untitled:a.cr");
    assert_eq!(response["version"], 11);
    assert_eq!(response["diagnostics"].as_array().unwrap().len(), 1);
    server.stop();
}

#[test]
fn reports_borrow_errors_using_the_same_frontend_as_the_compiler() {
    let mut server = Server::start();
    let response = server.open(
        "untitled:borrow.cr",
        1,
        "int main() { BoxInt a = box_int(1); BoxInt b = a; return 0; }",
    );
    assert!(
        response["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("cannot be copied")
    );
    server.stop();
}

#[test]
fn malformed_document_notifications_report_an_error_and_allow_recovery() {
    let mut server = Server::start();
    server.notify("textDocument/didOpen", json!({}));
    let Message::Notification(message) = server.receive() else {
        panic!("expected a log notification");
    };
    assert_eq!(message.method, "window/logMessage");
    let response = server.open("untitled:main.cr", 1, "int main() { return 0; }");
    assert_eq!(response["diagnostics"], json!([]));
    server.notify(
        "textDocument/didChange",
        json!({
            "textDocument": {"uri": "untitled:main.cr", "version": 2},
            "contentChanges": [{"range": {"start": {"line": 0, "character": 0}}, "text": "bad"}]
        }),
    );
    let Message::Notification(message) = server.receive() else {
        panic!("expected a log notification");
    };
    assert_eq!(message.method, "window/logMessage");
    server.change("untitled:main.cr", 3, "int main() { return 0; }");
    assert_eq!(server.diagnostics()["diagnostics"], json!([]));
    server.stop();
}

#[test]
fn shutdown_rejects_further_requests_and_exit_requires_shutdown() {
    let mut server = Server::start();
    assert_eq!(
        server.request("shutdown", json!({})).error.unwrap().code,
        -32602
    );
    server.shutdown();
    assert_eq!(
        server
            .request("test/afterShutdown", Value::Null)
            .error
            .unwrap()
            .code,
        -32600
    );
    server.exit(true);
    Server::start().exit(false);
}

#[test]
fn unsaved_imports_recheck_entries_and_keep_diagnostics_on_the_right_document() {
    let mut server = Server::start();
    let main = "file:///crust-lsp-unsaved-test/main.cr";
    let helper = "file:///crust-lsp-unsaved-test/helper.cr";
    let entry = "import \"helper.cr\"; int main() { return answer(); }";
    assert!(
        server.open(main, 1, entry)["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("could not read")
    );
    let result = server.open(helper, 1, "int answer() { return 42; }");
    assert_eq!(result["uri"], helper);
    assert_eq!(result["diagnostics"], json!([]));
    let result = server.diagnostics();
    assert_eq!(result["uri"], main);
    assert_eq!(result["diagnostics"], json!([]));

    server.change(helper, 2, "// 🦀\nint answer() { return missing; }");
    let result = server.diagnostics();
    assert_eq!(result["uri"], helper);
    assert_eq!(result["version"], 2);
    assert_eq!(
        result["diagnostics"][0]["range"]["start"],
        json!({"line": 1, "character": 22})
    );

    server.change(helper, 3, "int answer(int value) { return value; }");
    let result = server.diagnostics();
    assert_eq!(result["uri"], helper);
    assert_eq!(result["diagnostics"], json!([]));
    let result = server.diagnostics();
    assert_eq!(result["uri"], main);
    assert!(
        result["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("expects 1 argument")
    );

    server.change(helper, 4, "int answer() { return 42; }");
    assert_eq!(server.diagnostics()["diagnostics"], json!([]));
    let result = server.diagnostics();
    assert_eq!(result["uri"], main);
    assert_eq!(result["diagnostics"], json!([]));
    server.stop();
}

#[test]
fn opening_only_a_project_dependency_uses_the_entry_and_unsaved_dependency_text() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/lsp-project-tests")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("crust.toml"),
        "[project]\nname='example'\n[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n",
    )
    .unwrap();
    std::fs::write(
        root.join("main.cr"),
        "import \"helper.cr\"; int main() { return answer(); }",
    )
    .unwrap();
    std::fs::write(
        root.join("helper.cr"),
        "int answer() { return missing_on_disk; }",
    )
    .unwrap();
    let path = root.join("helper.cr").to_string_lossy().replace('\\', "/");
    let uri = if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    };
    let mut server = Server::start();
    let result = server.open(&uri, 1, "int answer() { return 42; }");
    assert_eq!(result["diagnostics"], json!([]));
    server.change(&uri, 2, "int answer() { return missing_in_editor; }");
    let result = server.diagnostics();
    assert_eq!(result["uri"], uri);
    assert!(
        result["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("missing_in_editor")
    );
    server.stop();
}

fn external_dependency_project(name: &str) -> (std::path::PathBuf, String, String, String) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/lsp-external-dependencies")
        .join(std::process::id().to_string())
        .join(name);
    let project = root.join("app");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("crust.toml"),
        "[project]\nname='external'\n[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n",
    )
    .unwrap();
    let main = "import \"../shared.cr\"; int supply(int value) { return value + 1; } int main() { return answer(41); }";
    std::fs::write(project.join("main.cr"), main).unwrap();
    std::fs::write(
        root.join("shared.cr"),
        "int answer(int value) { return missing_on_disk; }",
    )
    .unwrap();
    return (
        project.clone(),
        file_uri(&root.join("shared.cr")),
        file_uri(&project.join("main.cr")),
        main.to_owned(),
    );
}

#[test]
fn workspace_roots_supply_target_context_for_external_unsaved_dependencies() {
    let (project, uri, main_uri, main) = external_dependency_project("semantic");
    let source = "int answer(int value) { return supply(value); }";
    let mut server = Server::start_with_params(
        json!({"capabilities": {}, "workspaceFolders": [{"uri": file_uri(&project), "name": "app"}]}),
    );
    assert_eq!(server.open(&uri, 4, source)["diagnostics"], json!([]));
    let params = json!({"textDocument": {"uri": uri}, "position": point(source, "supply(value)")});
    let definition = server
        .request("textDocument/definition", params.clone())
        .result
        .unwrap();
    assert_eq!(definition[0]["uri"], main_uri);
    assert_eq!(definition[0]["range"]["start"], point(&main, "supply(int"));
    let hover = server.request("textDocument/hover", params).result.unwrap();
    assert!(
        hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("int supply(int value)")
    );
    assert!(
        complete(&mut server, &uri, point(source, "supply(value)"))
            .iter()
            .any(|item| item["label"] == "supply")
    );
    let signature = server.request("textDocument/signatureHelp", json!({"textDocument": {"uri": uri}, "position": at(source, source.find("supply(").unwrap() + "supply(".len())})).result.unwrap();
    assert_eq!(signature["signatures"][0]["label"], "int supply(int value)");
    assert!(!semantic_tokens(&mut server, &uri).is_empty());
    let renamed = server.request("textDocument/rename", json!({"textDocument": {"uri": uri}, "position": point(source, "answer(int"), "newName": "calculate"}));
    assert!(renamed.error.is_none(), "{:?}", renamed.error);
    let changes = renamed.result.unwrap()["changes"].clone();
    assert_eq!(changes[&uri][0]["newText"], "calculate");
    assert_eq!(changes[&main_uri][0]["newText"], "calculate");
    assert_eq!(changes.as_object().unwrap().len(), 2);
    server.stop();
}

#[test]
fn signature_help_preserves_volatile_qualifiers_on_pointer_slot_borrows() {
    let uri = "untitled:volatile-slots.cr";
    let source = "extern int* volatile native_slot; int* read_slot(int* volatile const^ slot) { return *slot; } int main() { unsafe { int* pointer = read_slot(^const native_slot); } return 0; }";
    let mut server = Server::start();
    assert_eq!(server.open(uri, 1, source)["diagnostics"], json!([]));
    let help = server.request("textDocument/signatureHelp", json!({"textDocument": {"uri": uri}, "position": at(source, source.find("read_slot(^").unwrap() + "read_slot(".len())})).result.unwrap();
    assert_eq!(
        help["signatures"][0]["label"],
        "int* read_slot(int* const volatile^ slot)"
    );
    server.stop();
}

#[test]
fn library_targets_support_unsaved_dependency_analysis_navigation_and_rename_without_main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/lsp-library")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("crust.toml"), "[project]\nname='library'\n[[targets]]\nname='library'\nkind='static-library'\nentry='library.cr'\n").unwrap();
    let library = "import \"helper.cr\"; extern int answer(int value) { return helper(value); }";
    let helper = "int helper(int value) { return value + 1; }";
    std::fs::write(root.join("library.cr"), library).unwrap();
    std::fs::write(
        root.join("helper.cr"),
        "int helper(int value) { return missing_on_disk; }",
    )
    .unwrap();
    let uri = file_uri(&root.join("library.cr"));
    let helper_uri = file_uri(&root.join("helper.cr"));
    let mut server =
        Server::start_with_params(json!({"capabilities": {}, "rootUri": file_uri(&root)}));
    assert_eq!(
        server.open(&helper_uri, 1, helper)["diagnostics"],
        json!([])
    );
    let renamed = server.request("textDocument/rename", json!({"textDocument": {"uri": helper_uri}, "position": point(helper, "helper(int"), "newName": "compute"}));
    assert!(renamed.error.is_none(), "{:?}", renamed.error);
    let changes = renamed.result.unwrap()["changes"].clone();
    assert_eq!(changes[&uri][0]["newText"], "compute");
    assert_eq!(changes[&helper_uri][0]["newText"], "compute");
    assert_eq!(workspace_symbols(&mut server, "answer").len(), 1);
    assert_eq!(server.open(&uri, 1, library)["diagnostics"], json!([]));
    let definition = server
        .request(
            "textDocument/definition",
            json!({"textDocument": {"uri": uri}, "position": point(library, "helper(value)")}),
        )
        .result
        .unwrap();
    assert_eq!(definition[0]["uri"], helper_uri);
    assert!(!semantic_tokens(&mut server, &uri).is_empty());
    assert!(
        complete(&mut server, &uri, point(library, "helper(value)"))
            .iter()
            .any(|item| item["label"] == "helper")
    );
    let help = server.request("textDocument/signatureHelp", json!({"textDocument": {"uri": uri}, "position": at(library, library.find("helper(").unwrap() + "helper(".len())})).result.unwrap();
    assert_eq!(help["signatures"][0]["label"], "int helper(int value)");
    server.change(&uri, 2, &format!("{library} int main() {{ return 0; }}"));
    let result = server.diagnostics();
    assert!(
        result["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("library targets cannot define main")
    );
    server.stop();
}

#[test]
fn excessive_nesting_reports_diagnostics_and_the_server_recovers() {
    let uri = "untitled:deep.cr";
    let mut server = Server::start();
    assert_eq!(
        server.open(uri, 1, "int main() { return 0; }")["diagnostics"],
        json!([])
    );
    for (index, source) in [
        format!("int main() {{ return {}1; }}", "1 + ".repeat(5000)),
        format!(
            "int main() {{ return {}0{}; }}",
            "(".repeat(2000),
            ")".repeat(2000)
        ),
        format!(
            "int main() {{{}return 0;{}}}",
            "{".repeat(2000),
            "}".repeat(2000)
        ),
    ]
    .iter()
    .enumerate()
    {
        let version = 2 + index as i32 * 2;
        server.change(uri, version, source);
        let result = server.diagnostics();
        assert!(
            result["diagnostics"][0]["message"]
                .as_str()
                .unwrap()
                .contains("nesting limit exceeded")
        );
        assert!(semantic_tokens(&mut server, uri).is_empty());
        server.change(uri, version + 1, "int main() { return 0; }");
        assert_eq!(server.diagnostics()["diagnostics"], json!([]));
        assert!(!semantic_tokens(&mut server, uri).is_empty());
    }
    server.stop();
}

#[test]
fn workspace_folder_changes_recheck_external_dependency_context() {
    let (project, uri, _, _) = external_dependency_project("folder-changes");
    let source = "int answer(int value) { return supply(value); }";
    let mut server =
        Server::start_with_params(json!({"capabilities": {}, "rootUri": file_uri(&project)}));
    assert_eq!(server.open(&uri, 1, source)["diagnostics"], json!([]));
    server.notify(
        "workspace/didChangeWorkspaceFolders",
        json!({"event": {"removed": [{"uri": file_uri(&project), "name": "app"}], "added": []}}),
    );
    let diagnostics = server.diagnostics();
    assert_eq!(diagnostics["uri"], uri);
    assert!(!diagnostics["diagnostics"].as_array().unwrap().is_empty());
    assert!(semantic_tokens(&mut server, &uri).is_empty());
    server.notify(
        "workspace/didChangeWorkspaceFolders",
        json!({"event": {"removed": [], "added": [{"uri": file_uri(&project), "name": "app"}]}}),
    );
    let diagnostics = server.diagnostics();
    assert_eq!(diagnostics["uri"], uri);
    assert_eq!(diagnostics["diagnostics"], json!([]));
    assert!(!semantic_tokens(&mut server, &uri).is_empty());
    server.stop();
}

#[test]
fn diagnostics_for_closed_dependencies_clear_when_the_import_is_removed() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/lsp-closed-tests")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("helper.cr"), "int answer() { return missing; }").unwrap();
    let path = root.join("main.cr").to_string_lossy().replace('\\', "/");
    let uri = if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    };
    let mut server = Server::start();
    assert_eq!(
        server.open(
            &uri,
            1,
            "import \"helper.cr\"; int main() { return answer(); }"
        )["diagnostics"],
        json!([])
    );
    let result = server.diagnostics();
    assert!(result["uri"].as_str().unwrap().ends_with("helper.cr"));
    assert!(
        result["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("missing")
    );
    server.change(&uri, 2, "int main() { return 0; }");
    assert_eq!(server.diagnostics()["uri"], uri);
    let result = server.diagnostics();
    assert!(result["uri"].as_str().unwrap().ends_with("helper.cr"));
    assert_eq!(result["diagnostics"], json!([]));
    server.stop();
}

fn point(source: &str, needle: &str) -> Value {
    return at(source, source.find(needle).unwrap());
}

fn at(source: &str, offset: usize) -> Value {
    let prefix = &source[..offset];
    return json!({ "line": prefix.bytes().filter(|byte| *byte == b'\n').count(), "character": prefix.rsplit('\n').next().unwrap().encode_utf16().count() });
}

fn complete(server: &mut Server, uri: &str, position: Value) -> Vec<Value> {
    let response = server.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": uri }, "position": position
        }),
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    return response.result.unwrap().as_array().unwrap().clone();
}

#[test]
fn completion_filters_prefixes_and_replaces_the_full_identifier_with_utf16_ranges() {
    let source = "int helper(int input) { return input; }\nint main() { /* 🦀 */ int value = 1; { int value = 2; print(value); int later = 3; } return 0; }";
    let uri = "untitled:completion.cr";
    let mut server = Server::start();
    assert_eq!(server.open(uri, 1, source)["diagnostics"], json!([]));
    let start = point(source, "value);");
    let mut middle = start.clone();
    middle["character"] = json!(start["character"].as_u64().unwrap() + 2);
    let items = complete(&mut server, uri, middle);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["label"], "value");
    assert_eq!(items[0]["kind"], 6);
    assert_eq!(items[0]["textEdit"]["range"]["start"], start);
    let mut end = start.clone();
    end["character"] = json!(start["character"].as_u64().unwrap() + 5);
    assert_eq!(items[0]["textEdit"]["range"]["end"], end);
    assert_eq!(items[0]["textEdit"]["newText"], "value");
    let items = complete(&mut server, uri, point(source, "print(value)"));
    assert!(items.iter().any(|item| item["label"] == "helper"));
    assert!(items.iter().any(|item| item["label"] == "value"));
    assert!(
        !items
            .iter()
            .any(|item| item["label"] == "later" || item["label"] == "input")
    );
    server.stop();
}

#[test]
fn completion_recovers_partial_expressions_and_unfinished_statements() {
    let uri = "untitled:partial-completion.cr";
    let mut server = Server::start();
    let source = "int main() { int value = 1; valu; return 0; }";
    assert_ne!(server.open(uri, 1, source)["diagnostics"], json!([]));
    let mut cursor = point(source, "valu;");
    cursor["character"] = json!(cursor["character"].as_u64().unwrap() + 4);
    let items = complete(&mut server, uri, cursor);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["label"], "value");

    let source = "int main() { int value = 1; val return 0; }";
    server.change(uri, 2, source);
    assert_ne!(server.diagnostics()["diagnostics"], json!([]));
    let mut cursor = point(source, "val return");
    cursor["character"] = json!(cursor["character"].as_u64().unwrap() + 3);
    let items = complete(&mut server, uri, cursor);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["label"], "value");
    assert_eq!(items[0]["kind"], 6);
    let source = "int main() { missing; pri; return 0; }";
    server.change(uri, 3, source);
    assert_ne!(server.diagnostics()["diagnostics"], json!([]));
    let mut cursor = point(source, "pri;");
    cursor["character"] = json!(cursor["character"].as_u64().unwrap() + 3);
    let items = complete(&mut server, uri, cursor);
    assert!(
        items
            .iter()
            .any(|item| item["label"] == "print" && item["kind"] == 3)
    );
    assert!(items.iter().any(|item| item["label"] == "private"));
    server.stop();
}

#[test]
fn completion_ignores_comments_strings_and_invalid_positions() {
    let source = "struct Item { int value; }; int main() { Item item = { .value = 1 }; /* comment */ print(\"string\"); // line\n print(item.value); return 0; }";
    let uri = "untitled:completion-sites.cr";
    let mut server = Server::start();
    assert_eq!(server.open(uri, 1, source)["diagnostics"], json!([]));
    for needle in ["mment", "ring", "ne\n"] {
        assert!(
            complete(&mut server, uri, point(source, needle)).is_empty(),
            "{needle}"
        );
    }
    let response = server.request(
        "textDocument/completion",
        json!({
            "textDocument": {"uri": uri}, "position": {"line": "invalid", "character": 0}
        }),
    );
    assert_eq!(response.error.unwrap().code, -32602);
    assert!(complete(&mut server, uri, json!({"line": 1000, "character": 0})).is_empty());
    server.stop();
}

#[test]
fn completion_uses_imported_declarations_and_current_unsaved_dependency_text() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/lsp-completion-imports")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&root).unwrap();
    let helper = root.join("helper.cr");
    std::fs::write(&helper, "int imported(int parameter) { return parameter; }").unwrap();
    let source = "import \"helper.cr\"; int main() { return 0; }";
    let uri = file_uri(&root.join("main.cr"));
    let helper_uri = file_uri(&helper);
    let mut server = Server::start();
    server.open(&uri, 1, source);
    let items = complete(&mut server, &uri, point(source, "return 0"));
    assert!(items.iter().any(|item| item["label"] == "imported"));
    assert!(!items.iter().any(|item| item["label"] == "parameter"));
    server.open(
        &helper_uri,
        1,
        "int updated(int argument) { return argument; }",
    );
    let items = complete(&mut server, &uri, point(source, "return 0"));
    assert!(items.iter().any(|item| item["label"] == "updated"));
    assert!(
        !items
            .iter()
            .any(|item| item["label"] == "imported" || item["label"] == "argument")
    );
    server.stop();
}

#[test]
fn member_completion_handles_incomplete_fields_methods_enum_values_and_native_pointers() {
    let definitions = r#"
        enum Status { Ready, Failed };
        typedef Status State;
        struct Item {
            private int hidden;
            int value;
            static Self create() { return (Self){ .hidden = 1, .value = 2 }; }
            int get() const { return this->hidden; }
        };
        struct Holder { Item item; };
    "#;
    let uri = "untitled:member-completion.cr";
    let mut server = Server::start();
    for (version, (body, needle, expected)) in [
        ("Item item = Item::create(); item.; return 0;", "item.", vec!["get", "value"]),
        ("Item item = Item::create(); item.va; return 0;", "item.va", vec!["value"]),
        ("Item::; return 0;", "Item::", vec!["create"]),
        ("Status::; return 0;", "Status::", vec!["Failed", "Ready"]),
        ("State::; return 0;", "State::", vec!["Failed", "Ready"]),
        ("Item item = Item::create(); const Item^ reference = ^const item; reference->; return 0;", "reference->", vec!["get", "value"]),
        ("Item item = Item::create(); unsafe { Item* raw = &item; raw->; } return 0;", "raw->", vec!["get", "value"]),
        ("Holder holder = { .item = Item::create() }; holder.item.; return 0;", "holder.item.", vec!["get", "value"]),
        ("Item::create().; return 0;", "Item::create().", vec!["get", "value"]),
    ].into_iter().enumerate() {
        let source = format!("{definitions} int main() {{ {body} }}");
        if version == 0 {
            server.open(uri, 1, &source);
        } else {
            server.change(uri, version as i32 + 1, &source);
            server.diagnostics();
        }
        assert!(crust::check(&source).is_err());
        assert!(crust::compile(&source).is_err());
        let cursor = source.rfind(needle).unwrap() + needle.len();
        let items = complete(&mut server, uri, at(&source, cursor));
        assert_eq!(items.iter().map(|item| item["label"].as_str().unwrap()).collect::<Vec<_>>(), expected, "{needle}");
    }
    server.stop();
}

#[test]
fn member_completion_preserves_private_method_context_and_generic_instantiations() {
    let mut server = Server::start();
    let uri = "untitled:private-generic-completion.cr";
    let source = "struct Item { private int hidden; int get() const { return this->; } }; int main() { return 0; }";
    server.open(uri, 1, source);
    let cursor = source.find("this->").unwrap() + "this->".len();
    let items = complete(&mut server, uri, at(source, cursor));
    assert_eq!(
        items
            .iter()
            .map(|item| item["label"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["get", "hidden"]
    );
    let source = "struct Cell<T> { T value; static Self create(T value) { return (Self){ .value = value }; } T get() const { return this->value; } }; int main() { Cell<int>::; return 0; }";
    server.change(uri, 2, source);
    server.diagnostics();
    let cursor = source.find("Cell<int>::").unwrap() + "Cell<int>::".len();
    let items = complete(&mut server, uri, at(source, cursor));
    assert_eq!(
        items
            .iter()
            .map(|item| item["label"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["create"]
    );
    server.stop();
}

#[test]
fn completion_triggers_distinguish_member_operators_from_comparisons_and_single_colons() {
    let source = "enum Status { Ready }; struct Item { int value; }; int main() { Item item = { .value = 1 }; const Item^ reference = ^const item; print(reference->value); print(item.value); print((int)Status::Ready); if (1 > 0) { print(1); } return 0; }";
    let uri = "untitled:trigger-completion.cr";
    let mut server = Server::start();
    assert_eq!(server.open(uri, 1, source)["diagnostics"], json!([]));
    for (needle, trigger, expected) in [
        ("reference->", ">", vec!["value"]),
        ("item.", ".", vec!["value"]),
        ("Status::", ":", vec!["Ready"]),
        ("Status:", ":", vec![]),
        ("1 >", ">", vec![]),
    ] {
        let cursor = source.find(needle).unwrap() + needle.len();
        let response = server.request(
            "textDocument/completion",
            json!({
                "textDocument": {"uri": uri}, "position": at(source, cursor),
                "context": {"triggerKind": 2, "triggerCharacter": trigger}
            }),
        );
        assert!(response.error.is_none());
        let result = response.result.unwrap();
        let items = result.as_array().unwrap();
        assert_eq!(
            items
                .iter()
                .map(|item| item["label"].as_str().unwrap())
                .collect::<Vec<_>>(),
            expected,
            "{needle}"
        );
    }
    server.stop();
}

fn workspace_symbols(server: &mut Server, query: &str) -> Vec<Value> {
    let response = server.request("workspace/symbol", json!({ "query": query }));
    assert!(response.error.is_none(), "{:?}", response.error);
    return response.result.unwrap().as_array().unwrap().clone();
}

#[test]
fn workspace_symbols_search_unsaved_names_and_containers_and_exclude_local_bindings() {
    let uri = "untitled:workspace-symbols.cr";
    let source = "// 🦀\r\nextern int native(int); extern int native(int); struct Item { int field; int get(int parameter) const { int local = parameter; return local; } }; int main() { return 0; }";
    let mut server = Server::start();
    assert_eq!(server.open(uri, 1, source)["diagnostics"], json!([]));
    let items = workspace_symbols(&mut server, "ITEM::GE");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"], "get");
    assert_eq!(items[0]["kind"], 6);
    assert_eq!(items[0]["containerName"], "Item");
    assert_eq!(items[0]["location"]["uri"], uri);
    assert_eq!(
        items[0]["location"]["range"]["start"],
        point(source, "get(int")
    );
    let items = workspace_symbols(&mut server, "");
    assert_eq!(
        items.iter().filter(|item| item["name"] == "native").count(),
        1
    );
    assert!(
        !items
            .iter()
            .any(|item| item["name"] == "local" || item["name"] == "parameter")
    );
    let response = server.request("workspace/symbol", json!({"query": 12}));
    assert_eq!(response.error.unwrap().code, -32602);
    server.change(
        uri,
        2,
        "int updated() { return 1; } int main() { return updated(); }",
    );
    server.diagnostics();
    assert_eq!(workspace_symbols(&mut server, "updated").len(), 1);
    assert!(workspace_symbols(&mut server, "Item").is_empty());
    server.stop();
}

#[test]
fn workspace_symbols_load_unopened_project_targets_and_deduplicate_shared_dependencies() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/lsp-workspace-symbols")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("crust.toml"), "[project]\nname = 'symbols'\n[[targets]]\nname = 'first'\nkind = 'executable'\nentry = 'first.cr'\n[[targets]]\nname = 'second'\nkind = 'executable'\nentry = 'second.cr'\n").unwrap();
    std::fs::write(
        root.join("first.cr"),
        "import \"shared.cr\"; int first() { return 1; } int main() { return first(); }",
    )
    .unwrap();
    std::fs::write(
        root.join("second.cr"),
        "import \"shared.cr\"; int second() { return 2; } int main() { return second(); }",
    )
    .unwrap();
    std::fs::write(
        root.join("shared.cr"),
        "struct Shared { int value; }; typedef Shared Alias;",
    )
    .unwrap();
    let uri = file_uri(&root);
    let mut server = Server::start_with_params(json!({
        "capabilities": {}, "workspaceFolders": [{"name": "symbols", "uri": uri}]
    }));
    assert_eq!(workspace_symbols(&mut server, "first").len(), 1);
    assert_eq!(workspace_symbols(&mut server, "second").len(), 1);
    let items = workspace_symbols(&mut server, "Shared");
    assert_eq!(
        items.iter().filter(|item| item["name"] == "Shared").count(),
        1
    );
    assert_eq!(
        items.iter().filter(|item| item["name"] == "value").count(),
        1
    );
    assert_eq!(
        items[0]["location"]["uri"],
        file_uri(&root.join("shared.cr"))
    );
    let shared_uri = file_uri(&root.join("shared.cr"));
    server.open(
        &shared_uri,
        1,
        "struct Updated { int value; }; typedef Updated Alias;",
    );
    assert!(workspace_symbols(&mut server, "Shared").is_empty());
    assert_eq!(
        workspace_symbols(&mut server, "Updated")
            .iter()
            .filter(|item| item["name"] == "Updated")
            .count(),
        1
    );
    server.stop();
}

#[test]
fn workspace_folder_changes_update_symbol_search_without_opening_documents() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/lsp-workspace-folders")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("crust.toml"), "[project]\nname = 'folder'\n[[targets]]\nname = 'app'\nkind = 'executable'\nentry = 'main.cr'\n").unwrap();
    std::fs::write(
        root.join("main.cr"),
        "int searchable() { return 1; } int main() { return searchable(); }",
    )
    .unwrap();
    let uri = file_uri(&root);
    let mut server = Server::start_with_params(json!({"capabilities": {}, "rootUri": uri}));
    assert_eq!(workspace_symbols(&mut server, "searchable").len(), 1);
    server.notify(
        "workspace/didChangeWorkspaceFolders",
        json!({"event": {
            "removed": [{"uri": uri, "name": "folder"}], "added": []
        }}),
    );
    assert!(workspace_symbols(&mut server, "searchable").is_empty());
    server.notify(
        "workspace/didChangeWorkspaceFolders",
        json!({"event": {
            "removed": [], "added": [{"uri": uri, "name": "folder"}]
        }}),
    );
    assert_eq!(workspace_symbols(&mut server, "searchable").len(), 1);
    server.stop();
}

#[test]
fn signature_help_tracks_nested_calls_partial_arguments_methods_callbacks_and_variadics() {
    let definitions = "int sum(int left, int right) { return left + right; } int lone(int value) { return value; } typedef int (*Callback)(int, int); extern int native(int tag, ...); struct Item { int value; int get(int amount) const { return this->value + amount; } };";
    let uri = "untitled:signatures.cr";
    let mut server = Server::start();
    for (version, (body, needle, expected, argument)) in [
        ("return sum();", "sum(", "int sum(int left, int right)", 0),
        (
            "return sum(1, );",
            "sum(1, ",
            "int sum(int left, int right)",
            1,
        ),
        ("return sum(1, lone(2));", "lone(", "int lone(int value)", 0),
        (
            "return sum(1, lone(2));",
            "lone(2)",
            "int sum(int left, int right)",
            1,
        ),
        (
            "Item item = { .value = 1 }; return item.get();",
            "item.get(",
            "int Item.get(int amount) const",
            0,
        ),
        (
            "Callback cb = sum; unsafe { return cb(); }",
            "cb(",
            "cb(int, int)",
            0,
        ),
        (
            "unsafe { return native(1, 2, 3); }",
            "native(1, 2, ",
            "extern int native(int tag, ...)",
            1,
        ),
        (
            "return sum(1, missing);",
            "sum(1, ",
            "int sum(int left, int right)",
            1,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let source = format!("{definitions} int main() {{ {body} }}");
        if version == 0 {
            server.open(uri, 1, &source);
        } else {
            server.change(uri, version as i32 + 1, &source);
            server.diagnostics();
        }
        let cursor = source.rfind(needle).unwrap() + needle.len();
        let response = server.request(
            "textDocument/signatureHelp",
            json!({
                "textDocument": {"uri": uri}, "position": at(&source, cursor)
            }),
        );
        assert!(response.error.is_none());
        let result = response.result.unwrap_or(Value::Null);
        assert!(
            result["signatures"][0]["label"]
                .as_str()
                .is_some_and(|label| label.ends_with(expected)),
            "{needle}: {result}"
        );
        assert_eq!(result["activeParameter"], argument, "{needle}");
        let signature = &result["signatures"][0];
        let range = &signature["parameters"][argument as usize]["label"];
        assert!(range[0].as_u64().unwrap() < range[1].as_u64().unwrap());
    }
    let response = server.request(
        "textDocument/signatureHelp",
        json!({"textDocument": {"uri": uri}, "position": {"line": 1000, "character": 0}}),
    );
    assert_eq!(response.result.unwrap_or(Value::Null), Value::Null);
    let response = server.request("textDocument/signatureHelp", json!({"position": null}));
    assert_eq!(response.error.unwrap().code, -32602);
    server.stop();
}

#[test]
fn signature_help_uses_current_unsaved_imported_parameter_types() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/lsp-signatures")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&root).unwrap();
    let helper = root.join("helper.cr");
    std::fs::write(&helper, "int convert(int value) { return value; }").unwrap();
    let source = "import \"helper.cr\"; int main() { return convert(); }";
    let uri = file_uri(&root.join("main.cr"));
    let mut server = Server::start();
    server.open(&uri, 1, source);
    let params = json!({"textDocument": {"uri": uri}, "position": at(source, source.find("convert(").unwrap() + 8)});
    let response = server.request("textDocument/signatureHelp", params.clone());
    assert_eq!(
        response.result.unwrap()["signatures"][0]["label"],
        "int convert(int value)"
    );
    server.open(
        &file_uri(&helper),
        1,
        "int convert(char byte) { return byte; }",
    );
    let response = server.request("textDocument/signatureHelp", params);
    assert_eq!(
        response.result.unwrap()["signatures"][0]["label"],
        "int convert(char byte)"
    );
    server.stop();
}

fn semantic_tokens(server: &mut Server, uri: &str) -> Vec<[u64; 5]> {
    let response = server.request(
        "textDocument/semanticTokens/full",
        json!({"textDocument": {"uri": uri}}),
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    let result = response.result.unwrap();
    let data = result["data"].as_array().unwrap();
    assert_eq!(data.len() % 5, 0);
    let (mut line, mut column) = (0, 0);
    return data
        .chunks_exact(5)
        .map(|token| {
            let delta_line = token[0].as_u64().unwrap();
            line += delta_line;
            column = if delta_line == 0 {
                column + token[1].as_u64().unwrap()
            } else {
                token[1].as_u64().unwrap()
            };
            return [
                line,
                column,
                token[2].as_u64().unwrap(),
                token[3].as_u64().unwrap(),
                token[4].as_u64().unwrap(),
            ];
        })
        .collect();
}

#[test]
fn semantic_tokens_classify_bound_names_and_builtins_with_utf16_positions() {
    let source = "typedef int Count;\r\nenum Status { Ready };\r\nstruct Item { int field; int get(int amount) const { return this->field + amount; } };\r\nT identity<T>(T value) { return value; }\r\nint main() { /* 🦀 */ Count count = 1; Item item = { .field = count }; int int64_t = 5; print(item.get(identity<int>(count))); print(Status::Ready == Ready); print(int64_t); return 0; }";
    let uri = "untitled:semantic-tokens.cr";
    let mut server = Server::start();
    assert_eq!(server.open(uri, 1, source)["diagnostics"], json!([]));
    let tokens = semantic_tokens(&mut server, uri);
    for (needle, advance, kind, modifiers) in [
        ("Count;", 0, 0, 1),
        ("Count count", 0, 0, 0),
        ("Status {", 0, 2, 1),
        ("Ready };", 0, 8, 1),
        ("Item {", 0, 1, 1),
        ("field;", 0, 7, 1),
        ("get(int", 0, 6, 1),
        ("amount)", 0, 9, 1),
        ("this->field", 0, 9, 2),
        ("this->field", 6, 7, 0),
        ("identity<T>", 0, 5, 1),
        ("identity<T>", 9, 4, 1),
        ("T value", 0, 4, 0),
        ("count = 1", 0, 10, 1),
        ("item.get", 0, 10, 0),
        ("item.get", 5, 6, 0),
        ("print(item", 0, 5, 2),
        ("identity<int>", 0, 5, 0),
        ("identity<int>", 9, 0, 2),
        ("Status::Ready", 0, 2, 0),
        ("Status::Ready", 8, 8, 0),
        ("int64_t =", 0, 10, 1),
        ("int64_t);", 0, 10, 0),
    ] {
        let location = at(source, source.find(needle).unwrap() + advance);
        let token = tokens
            .iter()
            .find(|token| {
                token[0] == location["line"].as_u64().unwrap()
                    && token[1] == location["character"].as_u64().unwrap()
            })
            .unwrap_or_else(|| panic!("missing token for {needle} + {advance}"));
        assert_eq!((token[3], token[4]), (kind, modifiers), "{needle}");
        assert!(token[2] > 0);
    }
    server.stop();
}

#[test]
fn semantic_tokens_clear_after_errors_and_close_and_recover_with_unsaved_text() {
    let uri = "untitled:semantic-recovery.cr";
    let mut server = Server::start();
    server.open(uri, 1, "int main() { return 0; }");
    assert!(!semantic_tokens(&mut server, uri).is_empty());
    server.change(uri, 2, "int main() { return missing; }");
    server.diagnostics();
    assert!(semantic_tokens(&mut server, uri).is_empty());
    server.change(uri, 3, "int main() { int value = 1; return value; }");
    server.diagnostics();
    assert!(!semantic_tokens(&mut server, uri).is_empty());
    server.notify(
        "textDocument/didClose",
        json!({"textDocument": {"uri": uri}}),
    );
    server.diagnostics();
    assert!(semantic_tokens(&mut server, uri).is_empty());
    let response = server.request(
        "textDocument/semanticTokens/full",
        json!({"textDocument": {}}),
    );
    assert_eq!(response.error.unwrap().code, -32602);
    server.stop();
}

#[test]
fn semantic_tokens_follow_imported_type_and_function_declarations() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/lsp-semantic-tokens")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("helper.cr"),
        "struct Shared { int value; }; int read(const Shared^ item) { return item->value; }",
    )
    .unwrap();
    let source = "import \"helper.cr\"; int main() { Shared item = { .value = 1 }; return read(^const item); }";
    let uri = file_uri(&root.join("main.cr"));
    let mut server = Server::start();
    assert_eq!(server.open(&uri, 1, source)["diagnostics"], json!([]));
    let tokens = semantic_tokens(&mut server, &uri);
    for (needle, kind) in [("Shared item", 1), ("read(^const", 5), ("value = 1", 7)] {
        let location = point(source, needle);
        let token = tokens
            .iter()
            .find(|token| {
                token[0] == location["line"].as_u64().unwrap()
                    && token[1] == location["character"].as_u64().unwrap()
            })
            .unwrap();
        assert_eq!((token[3], token[4]), (kind, 0), "{needle}");
    }
    server.stop();
}

#[test]
fn semantic_tokens_request_supported_client_refreshes_after_document_updates() {
    let uri = "untitled:semantic-refresh.cr";
    let mut server = Server::start_with_capabilities(
        json!({"workspace": {"semanticTokens": {"refreshSupport": true}}}),
    );
    server.open(uri, 1, "int main() { return 0; }");
    let Message::Request(request) = server.receive() else {
        panic!("expected a semantic-token refresh request");
    };
    assert_eq!(request.method, "workspace/semanticTokens/refresh");
    Message::Response(Response::new_ok(request.id, Value::Null))
        .write(&mut server.input)
        .unwrap();
    assert!(!semantic_tokens(&mut server, uri).is_empty());
    server.change(uri, 2, "int main() { int value = 1; return value; }");
    server.diagnostics();
    let Message::Request(request) = server.receive() else {
        panic!("expected a semantic-token refresh request");
    };
    assert_eq!(request.method, "workspace/semanticTokens/refresh");
    Message::Response(Response::new_ok(request.id, Value::Null))
        .write(&mut server.input)
        .unwrap();
    assert!(!semantic_tokens(&mut server, uri).is_empty());
    server.stop();
}

fn file_uri(path: &std::path::Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    let mut uri = if path.starts_with('/') {
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

#[test]
fn hover_definition_and_document_symbols_use_resolved_bindings_and_utf16_positions() {
    let source = "// 🦀\r\nint answer(int input) { int value = input; { int value = 7; print(value); } return value; }\r\nint main() { return answer(42); }";
    let mut server = Server::start();
    let uri = "untitled:semantic.cr";
    assert_eq!(server.open(uri, 1, source)["diagnostics"], json!([]));
    let hover = server
        .request(
            "textDocument/hover",
            json!({"textDocument": {"uri": uri}, "position": point(source, "value);")}),
        )
        .result
        .unwrap();
    assert_eq!(hover["contents"]["value"], "```crust\nint value\n```");
    let definition = server
        .request(
            "textDocument/definition",
            json!({"textDocument": {"uri": uri}, "position": point(source, "value);")}),
        )
        .result
        .unwrap();
    assert_eq!(definition[0]["uri"], uri);
    assert_eq!(definition[0]["range"]["start"], point(source, "value = 7"));
    let hover = server
        .request(
            "textDocument/hover",
            json!({"textDocument": {"uri": uri}, "position": point(source, "answer(42)")}),
        )
        .result
        .unwrap();
    assert_eq!(
        hover["contents"]["value"],
        "```crust\nint answer(int input)\n```"
    );
    let hover = server
        .request(
            "textDocument/hover",
            json!({"textDocument": {"uri": uri}, "position": point(source, "42")}),
        )
        .result
        .unwrap();
    assert_eq!(hover["contents"]["value"], "```crust\nint\n```");
    let symbols = server
        .request(
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": uri}}),
        )
        .result
        .unwrap();
    assert_eq!(symbols.as_array().unwrap().len(), 2);
    assert_eq!(symbols[0]["name"], "answer");
    assert_eq!(symbols[0]["kind"], 12);
    assert_eq!(symbols[1]["name"], "main");
    server.stop();
}

#[test]
fn navigation_and_hover_follow_unsaved_imports_and_report_closed_target_files() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/lsp semantic ü")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("crust.toml"),
        "[project]\nname='example'\n[[targets]]\nname='app'\nkind='executable'\nentry='main.cr'\n",
    )
    .unwrap();
    let source = "import \"helper.cr\"; int main() { return answer(42); }";
    let helper = "// 🦀\r\nint answer(int input) { return input; }";
    std::fs::write(root.join("main.cr"), source).unwrap();
    std::fs::write(root.join("helper.cr"), helper).unwrap();
    let main_uri = file_uri(&root.join("main.cr"));
    let helper_uri = file_uri(&root.join("helper.cr"));
    let mut server = Server::start();
    assert_eq!(server.open(&main_uri, 1, source)["diagnostics"], json!([]));
    let query = json!({"textDocument": {"uri": main_uri}, "position": point(source, "answer(42)")});
    let result = server
        .request("textDocument/definition", query.clone())
        .result
        .unwrap();
    assert_eq!(result[0]["uri"], helper_uri);
    assert_eq!(result[0]["range"]["start"], point(helper, "answer(int"));
    let unsaved = "// changed\n// 🦀\nint answer(int renamed) { return renamed; }";
    assert_eq!(
        server.open(&helper_uri, 1, unsaved)["diagnostics"],
        json!([])
    );
    let result = server
        .request("textDocument/definition", query.clone())
        .result
        .unwrap();
    assert_eq!(result[0]["range"]["start"], point(unsaved, "answer(int"));
    let result = server.request("textDocument/hover", query).result.unwrap();
    assert_eq!(
        result["contents"]["value"],
        "```crust\nint answer(int renamed)\n```"
    );
    server.stop();
}

#[test]
fn semantic_requests_validate_parameters_and_recover_after_compiler_errors() {
    let mut server = Server::start();
    let uri = "untitled:invalid-semantic.cr";
    let broken = "int main() { return missing; }";
    server.open(uri, 1, broken);
    let query = json!({"textDocument": {"uri": uri}, "position": {"line": 0, "character": 4}});
    let response = server.request("textDocument/hover", query.clone());
    assert!(response.error.is_none());
    assert!(response.result.unwrap_or(Value::Null).is_null());
    assert_eq!(
        server
            .request("textDocument/hover", json!({}))
            .error
            .unwrap()
            .code,
        -32602
    );
    server.change(uri, 2, "int main() { return 0; }");
    assert_eq!(server.diagnostics()["diagnostics"], json!([]));
    assert!(
        !server
            .request("textDocument/hover", query)
            .result
            .unwrap()
            .is_null()
    );
    server.stop();
}

#[test]
fn references_follow_bindings_and_redeclarations_with_declaration_control() {
    let source = "// 🦀\r\nextern int call(int); extern int call(int); int main() { int value = 1; { int value = 2; print(value); } unsafe { return call(value); } }";
    let uri = "untitled:references.cr";
    let mut server = Server::start();
    assert_eq!(server.open(uri, 1, source)["diagnostics"], json!([]));
    for include in [false, true] {
        let result = server
            .request(
                "textDocument/references",
                json!({
                    "textDocument": {"uri": uri}, "position": point(source, "call(int); int main"),
                    "context": {"includeDeclaration": include}
                }),
            )
            .result
            .unwrap();
        assert_eq!(
            result.as_array().unwrap().len(),
            if include { 3 } else { 1 }
        );
        let last = result.as_array().unwrap().last().unwrap();
        assert_eq!(last["uri"], uri);
        assert_eq!(last["range"]["start"], point(source, "call(value)"));
    }
    let result = server
        .request(
            "textDocument/references",
            json!({
                "textDocument": {"uri": uri}, "position": point(source, "value = 2"),
                "context": {"includeDeclaration": false}
            }),
        )
        .result
        .unwrap();
    assert_eq!(result.as_array().unwrap().len(), 1);
    assert_eq!(result[0]["range"]["start"], point(source, "value);"));
    let malformed = server.request(
        "textDocument/references",
        json!({
            "textDocument": {"uri": uri}, "position": point(source, "value = 2")
        }),
    );
    assert_eq!(malformed.error.unwrap().code, -32602);
    server.stop();
}

#[test]
fn references_use_unsaved_imports_and_closed_source_files() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/lsp-references")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&root).unwrap();
    let entry = "import \"helper.cr\"; int main() { return answer(1) + answer(2); }";
    let helper = "int answer(int input) { return input; }";
    std::fs::write(root.join("main.cr"), entry).unwrap();
    std::fs::write(root.join("helper.cr"), helper).unwrap();
    let uri = file_uri(&root.join("main.cr"));
    let helper_uri = file_uri(&root.join("helper.cr"));
    let mut server = Server::start();
    assert_eq!(server.open(&uri, 1, entry)["diagnostics"], json!([]));
    let query = json!({"textDocument": {"uri": uri}, "position": point(entry, "answer(1)"), "context": {"includeDeclaration": true}});
    let result = server
        .request("textDocument/references", query.clone())
        .result
        .unwrap();
    assert_eq!(result.as_array().unwrap().len(), 3);
    assert!(
        result
            .as_array()
            .unwrap()
            .iter()
            .any(|location| location["uri"] == helper_uri)
    );
    let unsaved = "// 🦀\r\nint answer(int input) { return input; }";
    assert_eq!(
        server.open(&helper_uri, 1, unsaved)["diagnostics"],
        json!([])
    );
    let result = server
        .request("textDocument/references", query)
        .result
        .unwrap();
    let declaration = result
        .as_array()
        .unwrap()
        .iter()
        .find(|location| location["uri"] == helper_uri)
        .unwrap();
    assert_eq!(declaration["range"]["start"], point(unsaved, "answer(int"));
    server.stop();
}

#[test]
fn type_navigation_and_outlines_include_aliases_interfaces_and_unused_generics() {
    let source = "typedef int Count; interface Readable { int read() const; }; struct Unused<T: Readable> { T value; }; int main() { Count value = 42; return value; }";
    let uri = "untitled:types.cr";
    let mut server = Server::start();
    assert_eq!(server.open(uri, 1, source)["diagnostics"], json!([]));
    let result = server
        .request(
            "textDocument/definition",
            json!({"textDocument": {"uri": uri}, "position": point(source, "Count value")}),
        )
        .result
        .unwrap();
    assert_eq!(result[0]["range"]["start"], point(source, "Count;"));
    let result = server
        .request(
            "textDocument/hover",
            json!({"textDocument": {"uri": uri}, "position": point(source, "Count value")}),
        )
        .result
        .unwrap();
    assert_eq!(
        result["contents"]["value"],
        "```crust\ntypedef int Count\n```"
    );
    let result = server
        .request(
            "textDocument/definition",
            json!({"textDocument": {"uri": uri}, "position": point(source, "Readable>")}),
        )
        .result
        .unwrap();
    assert_eq!(result[0]["range"]["start"], point(source, "Readable {"));
    let symbols = server
        .request(
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": uri}}),
        )
        .result
        .unwrap();
    let symbols = symbols.as_array().unwrap();
    assert!(
        symbols
            .iter()
            .any(|symbol| symbol["name"] == "Unused" && symbol["kind"] == 23)
    );
    assert!(symbols.iter().any(|symbol| symbol["name"] == "Count"));
    assert!(
        symbols
            .iter()
            .any(|symbol| symbol["name"] == "Readable" && symbol["kind"] == 11)
    );
    assert!(
        symbols
            .iter()
            .any(|symbol| symbol["name"] == "read" && symbol["containerName"] == "Readable")
    );
    server.stop();
}

#[test]
fn external_variables_have_hover_navigation_references_and_source_outlines() {
    let uri = "untitled:external-variables.cr";
    let source = "extern int value; extern int value; int main() { unsafe { value = 42; print(value); } return 0; }";
    let mut server = Server::start();
    assert_eq!(server.open(uri, 1, source)["diagnostics"], json!([]));
    let query = json!({"textDocument": {"uri": uri}, "position": point(source, "value =")});
    let hover = server
        .request("textDocument/hover", query.clone())
        .result
        .unwrap();
    assert_eq!(
        hover["contents"]["value"],
        "```crust\nextern int value\n```"
    );
    let definition = server
        .request("textDocument/definition", query.clone())
        .result
        .unwrap();
    assert_eq!(definition[0]["range"]["start"], point(source, "value;"));
    let mut references = query;
    references["context"] = json!({"includeDeclaration": true});
    let references = server
        .request("textDocument/references", references)
        .result
        .unwrap();
    assert_eq!(references.as_array().unwrap().len(), 4);
    let symbols = server
        .request(
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": uri}}),
        )
        .result
        .unwrap();
    assert!(
        symbols
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["name"] == "value" && symbol["kind"] == 13)
    );
    server.stop();
}

#[test]
fn prepare_and_rename_return_identifier_ranges_and_client_compatible_edits() {
    let source = "/* 🦀 */ int main() { int value = 1; return (value); }";
    let uri = "untitled:rename.cr";
    let mut server = Server::start();
    assert_eq!(server.open(uri, 1, source)["diagnostics"], json!([]));
    let position = point(source, "value);");
    let prepared = server
        .request(
            "textDocument/prepareRename",
            json!({
                "textDocument": {"uri": uri}, "position": position
            }),
        )
        .result
        .unwrap();
    assert_eq!(prepared["placeholder"], "value");
    assert_eq!(prepared["range"]["start"], position);
    assert_eq!(
        prepared["range"]["end"]["character"].as_u64().unwrap(),
        position["character"].as_u64().unwrap() + 5
    );
    let result = server
        .request(
            "textDocument/rename",
            json!({
                "textDocument": {"uri": uri}, "position": position, "newName": "count"
            }),
        )
        .result
        .unwrap();
    let edits = result["changes"][uri].as_array().unwrap();
    assert_eq!(edits.len(), 2);
    assert!(edits.iter().all(|edit| edit["newText"] == "count"));
    assert_eq!(edits[0]["range"]["start"], point(source, "value ="));
    assert_eq!(edits[1]["range"]["start"], position);
    assert!(result.get("documentChanges").is_none());
    let unsupported = server.request(
        "textDocument/prepareRename",
        json!({
            "textDocument": {"uri": uri}, "position": point(source, "main()")
        }),
    );
    assert!(unsupported.result.unwrap_or(Value::Null).is_null());
    server.stop();
}

#[test]
fn rename_uses_current_document_versions_and_rejects_capture_and_invalid_requests() {
    let uri = "untitled:versioned-rename.cr";
    let mut server = Server::start_with_capabilities(json!({
        "workspace": {"workspaceEdit": {"documentChanges": true}}
    }));
    server.open(uri, 1, "int main() { return 0; }");
    let source = "int answer(int value) { return value; } int main() { return answer(1); }";
    server.change(uri, 7, source);
    assert_eq!(server.diagnostics()["diagnostics"], json!([]));
    let query = json!({"textDocument": {"uri": uri}, "position": point(source, "value;"), "newName": "input"});
    let result = server.request("textDocument/rename", query).result.unwrap();
    assert_eq!(
        result["documentChanges"][0]["textDocument"],
        json!({"uri": uri, "version": 7})
    );
    assert_eq!(
        result["documentChanges"][0]["edits"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(result.get("changes").is_none());
    let capture =
        "int main() { int original = 1; { int taken = 2; print(original); } return original; }";
    server.change(uri, 8, capture);
    assert_eq!(server.diagnostics()["diagnostics"], json!([]));
    let response = server.request("textDocument/rename", json!({
        "textDocument": {"uri": uri}, "position": point(capture, "original ="), "newName": "taken"
    }));
    assert_eq!(response.error.unwrap().code, -32803);
    let response = server.request(
        "textDocument/rename",
        json!({
            "textDocument": {"uri": uri}, "position": point(capture, "original =")
        }),
    );
    assert_eq!(response.error.unwrap().code, -32602);
    server.change(uri, 9, "int main() { return missing; }");
    assert!(
        !server.diagnostics()["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let response = server.request("textDocument/rename", json!({
        "textDocument": {"uri": uri}, "position": {"line": 0, "character": 4}, "newName": "entry"
    }));
    assert_eq!(response.error.unwrap().code, -32803);
    server.stop();
}

#[test]
fn function_and_external_variable_rename_return_native_alias_edits() {
    let uri = "untitled:native-rename.cr";
    let source = "/* 🦀 */ extern int native(int); extern const int native_value; int main() { unsafe { return native(native_value); } }";
    let mut server = Server::start();
    assert_eq!(server.open(uri, 1, source)["diagnostics"], json!([]));
    for (needle, new_name, native, semicolon) in [
        (
            "native(int)",
            "call_native",
            "native",
            source.find(';').unwrap(),
        ),
        (
            "native_value;",
            "count",
            "native_value",
            source.find("native_value;").unwrap() + "native_value".len(),
        ),
    ] {
        let result = server.request(
            "textDocument/rename",
            json!({
                "textDocument": {"uri": uri}, "position": point(source, needle), "newName": new_name
            }),
        );
        assert!(result.error.is_none(), "{:?}", result.error);
        let result = result.result.unwrap();
        let edits = result["changes"][uri].as_array().unwrap();
        assert_eq!(edits.len(), 3);
        let alias = edits
            .iter()
            .find(|edit| edit["newText"] == format!(" = \"{native}\""))
            .unwrap();
        assert_eq!(alias["range"]["start"], at(source, semicolon));
        assert_eq!(alias["range"]["end"], alias["range"]["start"]);
    }
    server.stop();
}

fn rename_project(name: &str) -> std::path::PathBuf {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/lsp-global-rename")
        .join(std::process::id().to_string())
        .join(name);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("crust.toml"), "[project]\nname = 'rename'\n[[targets]]\nname = 'first'\nkind = 'executable'\nentry = 'first.cr'\n[[targets]]\nname = 'second'\nkind = 'executable'\nentry = 'second.cr'\n").unwrap();
    return root;
}

#[test]
fn global_rename_updates_unopened_target_callers_and_returns_versioned_shared_edits() {
    let root = rename_project("shared ü");
    let first = "import \"shared.cr\"; int main() { return answer(); }";
    let second = "import \"shared.cr\"; int main() { return answer(); }";
    let shared = "int answer() { return 42; } extern const int native_value;";
    std::fs::write(root.join("first.cr"), first).unwrap();
    std::fs::write(root.join("second.cr"), second).unwrap();
    std::fs::write(root.join("shared.cr"), shared).unwrap();
    let first_uri = file_uri(&root.join("first.cr"));
    let second_uri = file_uri(&root.join("second.cr"));
    let shared_uri = file_uri(&root.join("shared.cr"));
    let mut server = Server::start_with_capabilities(
        json!({"workspace": {"workspaceEdit": {"documentChanges": true}}}),
    );
    assert_eq!(server.open(&first_uri, 7, first)["diagnostics"], json!([]));
    let response = server.request("textDocument/rename", json!({
        "textDocument": {"uri": first_uri}, "position": point(first, "answer()"), "newName": "calculate"
    }));
    assert!(response.error.is_none(), "{:?}", response.error);
    let result = response.result.unwrap();
    let changes = result["documentChanges"].as_array().unwrap();
    assert_eq!(changes.len(), 3);
    for uri in [&first_uri, &second_uri, &shared_uri] {
        let change = changes
            .iter()
            .find(|change| change["textDocument"]["uri"] == **uri)
            .unwrap();
        assert_eq!(change["edits"].as_array().unwrap().len(), 1);
        assert_eq!(change["edits"][0]["newText"], "calculate");
        if uri == &first_uri {
            assert_eq!(change["textDocument"]["version"], 7);
        } else {
            assert!(change["textDocument"]["version"].is_null());
        }
    }
    assert_eq!(
        std::fs::read_to_string(root.join("shared.cr")).unwrap(),
        shared
    );
    server.open(&shared_uri, 3, shared);
    let response = server.request("textDocument/rename", json!({
        "textDocument": {"uri": shared_uri}, "position": point(shared, "native_value;"), "newName": "count"
    }));
    assert!(response.error.is_none(), "{:?}", response.error);
    let result = response.result.unwrap();
    let changes = result["documentChanges"].as_array().unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0]["textDocument"]["version"], 3);
    assert_eq!(changes[0]["edits"].as_array().unwrap().len(), 2);
    server.stop();
}

#[test]
fn global_rename_checks_unopened_targets_for_capture_and_affected_errors() {
    let root = rename_project("capture");
    let first = "import \"shared.cr\"; int main() { return answer(1); }";
    std::fs::write(root.join("first.cr"), first).unwrap();
    std::fs::write(
        root.join("shared.cr"),
        "int answer(int value) { return value; }",
    )
    .unwrap();
    std::fs::write(
        root.join("second.cr"),
        "import \"shared.cr\"; int main() { int calculate = 1; return answer(calculate); }",
    )
    .unwrap();
    let uri = file_uri(&root.join("first.cr"));
    let mut server = Server::start();
    server.open(&uri, 1, first);
    let params = json!({"textDocument": {"uri": uri}, "position": point(first, "answer(1)"), "newName": "calculate"});
    let response = server.request("textDocument/rename", params.clone());
    assert_eq!(response.error.unwrap().code, -32803);
    std::fs::write(
        root.join("second.cr"),
        "import \"shared.cr\"; int main() { return missing; }",
    )
    .unwrap();
    let response = server.request("textDocument/rename", params);
    let error = response.error.unwrap();
    assert_eq!(error.code, -32803);
    assert!(
        error.message.contains("affected targets"),
        "{}",
        error.message
    );
    std::fs::write(root.join("crust.toml"), "[project]\nname = 'rename'\n[[targets]]\nname = 'first'\nkind = 'executable'\nentry = 'first.cr'\n[[targets]]\nname = 'second'\nkind = 'executable'\nentry = 'shared.cr'\n").unwrap();
    let response = server.request("textDocument/rename", json!({
        "textDocument": {"uri": uri}, "position": point(first, "answer(1)"), "newName": "calculate"
    }));
    assert!(response.error.unwrap().message.contains("affected targets"));
    server.stop();
}

#[test]
fn global_rename_rejects_shared_calls_bound_to_different_target_definitions() {
    let root = rename_project("different-bindings");
    let first = "import \"shared.cr\"; int answer() { return 1; } int main() { return forward(); }";
    std::fs::write(root.join("first.cr"), first).unwrap();
    std::fs::write(root.join("shared.cr"), "int forward() { return answer(); }").unwrap();
    std::fs::write(root.join("second.cr"), "import \"shared.cr\"; int answer() { return 2; } int calculate() { return 3; } int main() { return forward(); }").unwrap();
    let uri = file_uri(&root.join("first.cr"));
    let mut server = Server::start();
    server.open(&uri, 1, first);
    let response = server.request("textDocument/rename", json!({
        "textDocument": {"uri": uri}, "position": point(first, "answer()"), "newName": "calculate"
    }));
    let error = response.error.unwrap();
    assert_eq!(error.code, -32803);
    assert!(
        error.message.contains("which declaration"),
        "{}",
        error.message
    );
    server.stop();
}
