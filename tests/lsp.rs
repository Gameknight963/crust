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
        let initialized = server.request("initialize", json!({"capabilities": {}}));
        let capabilities = &initialized.result.unwrap()["capabilities"];
        assert_eq!(capabilities["positionEncoding"], "utf-16");
        assert_eq!(capabilities["textDocumentSync"]["change"], 1);
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
