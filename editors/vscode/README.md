# Crust for VS Code

- Compiler diagnostics for unsaved Crust files
- Syntax highlighting, bracket pairs, and comment commands
- Scope and member completion, plus signature help
- Hover, navigation, references, workspace search, and rename
- Semantic highlighting for resolved bindings
- Shared language server for VS Code and other LSP editors

## Setup

Build `crust-lsp` from the compiler checkout:

```powershell
cargo build --bin crust-lsp
```

Set `crust.server.path` to the executable, or put `crust-lsp` on `PATH`.
Open a `.cr` file to start the server. The **Crust** output channel contains
server logs. **Crust: Restart Language Server** restarts the connection.

## Build the extension

From this directory:

```powershell
npm ci
npm run package
```

Install `target/crust-language.vsix` using **Extensions: Install from VSIX**.

## Development

Run `npm ci` and `npm run build` in this directory, then open the compiler checkout in VS Code.
Launch **Crust: VS Code extension** to build the Rust binaries and open an
extension development window. Development sessions use the local debug server.
