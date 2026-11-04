"""Measure cold diagnostics and repeated editor requests against a real project."""

import argparse
import json
import os
from pathlib import Path
import queue
import statistics
import subprocess
import threading
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--server", type=Path, default=Path("target/release/crust-lsp" + (".exe" if os.name == "nt" else "")))
    parser.add_argument("--line", type=int, default=0, help="Zero-based line")
    parser.add_argument("--character", type=int, default=0, help="Zero-based UTF-16 column")
    parser.add_argument("--repeat", type=int, default=10)
    parser.add_argument("--max-warm-ms", type=float, help="Fail if any warm request exceeds this budget")
    parser.add_argument("--require-clean", action="store_true", help="Fail if initial diagnostics contain errors")
    args = parser.parse_args()
    if args.repeat < 1:
        parser.error("--repeat must be positive")
    source = args.source.resolve()
    manifest = next((p / "crust.toml" for p in source.parents if (p / "crust.toml").is_file()), None)
    root = manifest.parent if manifest else source.parent
    messages = queue.Queue()
    process = subprocess.Popen([str(args.server.resolve()), "--stdio"], stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)

    def receive():
        try:
            while True:
                headers = {}
                while True:
                    line = process.stdout.readline()
                    if not line:
                        raise EOFError("Language server closed its output")
                    if line == b"\r\n":
                        break
                    key, value = line.decode("ascii").split(":", 1)
                    headers[key.lower()] = value.strip()
                messages.put(json.loads(process.stdout.read(int(headers["content-length"]))))
        except Exception as error:
            messages.put(error)

    threading.Thread(target=receive, daemon=True).start()

    def send(method, params, request_id=None):
        message = {"jsonrpc": "2.0", "method": method, "params": params}
        if request_id is not None:
            message["id"] = request_id
        body = json.dumps(message).encode("utf-8")
        process.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode("ascii") + body)
        process.stdin.flush()

    def until(predicate):
        deadline = time.monotonic() + 30
        while True:
            message = messages.get(timeout=max(0.01, deadline - time.monotonic()))
            if isinstance(message, Exception):
                raise message
            if predicate(message):
                return message

    request_id = 0

    def request(method, params):
        nonlocal request_id
        request_id += 1
        start = time.perf_counter()
        send(method, params, request_id)
        result = until(lambda message: message.get("id") == request_id)
        if "error" in result:
            raise RuntimeError(result["error"])
        return (time.perf_counter() - start) * 1000, result.get("result")

    try:
        request("initialize", {"rootUri": root.as_uri(), "capabilities": {}})
        send("initialized", {})
        start = time.perf_counter()
        send("textDocument/didOpen", {"textDocument": {
            "uri": source.as_uri(), "languageId": "crust", "version": 1,
            "text": source.read_text(encoding="utf-8"),
        }})
        diagnostic = until(lambda message: message.get("method") == "textDocument/publishDiagnostics"
                           and message["params"]["uri"] == source.as_uri())
        report = {"source": str(source), "diagnostics_ms": round((time.perf_counter() - start) * 1000, 2),
                  "diagnostics": diagnostic["params"]["diagnostics"], "requests": {}}
        query = {"textDocument": {"uri": source.as_uri()},
                 "position": {"line": args.line, "character": args.character}}
        for method in ["textDocument/semanticTokens/full", "textDocument/hover", "textDocument/definition",
                       "textDocument/documentHighlight", "textDocument/signatureHelp", "textDocument/foldingRange"]:
            samples = [request(method, query)[0] for _ in range(args.repeat)]
            report["requests"][method] = {"median_ms": round(statistics.median(samples), 2),
                                          "max_ms": round(max(samples), 2)}
        print(json.dumps(report, indent=2))
        if args.require_clean and any(d.get("severity") == 1 for d in report["diagnostics"]):
            raise SystemExit("Benchmark fixture has compiler errors")
        if args.max_warm_ms is not None and any(
            value["max_ms"] > args.max_warm_ms for value in report["requests"].values()
        ):
            raise SystemExit("Warm editor request exceeded the latency budget")
        request("shutdown", None)
        send("exit", None)
        process.wait(timeout=5)
    finally:
        if process.poll() is None:
            process.kill()
        process.wait()


if __name__ == "__main__":
    main()
