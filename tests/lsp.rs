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
        let initialized = server.request("initialize", json!({"capabilities": capabilities}));
        let capabilities = &initialized.result.unwrap()["capabilities"];
        assert_eq!(capabilities["positionEncoding"], "utf-16");
        assert_eq!(capabilities["textDocumentSync"]["change"], 1);
        assert_eq!(capabilities["hoverProvider"], true);
        assert_eq!(capabilities["definitionProvider"], true);
        assert_eq!(capabilities["referencesProvider"], true);
        assert_eq!(capabilities["renameProvider"]["prepareProvider"], true);
        assert_eq!(capabilities["documentSymbolProvider"], true);
        assert_eq!(capabilities["completionProvider"]["resolveProvider"], false);
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
    let prefix = &source[..source.find(needle).unwrap()];
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
fn completion_ignores_comments_strings_and_qualified_member_sites() {
    let source = "struct Item { int value; }; int main() { Item item = { .value = 1 }; /* comment */ print(\"string\"); // line\n print(item.value); return 0; }";
    let uri = "untitled:completion-sites.cr";
    let mut server = Server::start();
    assert_eq!(server.open(uri, 1, source)["diagnostics"], json!([]));
    for needle in ["mment", "ring", "ne\n", "value);"] {
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
