# Crust for VS Code

- Compiler diagnostics for unsaved Crust files
- Syntax highlighting, bracket pairs, and comment commands
- Scope and member completion, plus signature help
- Hover, navigation, references, workspace search, and rename
- Semantic highlighting for resolved bindings
- Shared language server for VS Code and other LSP editors
- Build, check, and run tasks for `crust.toml` projects

## Setup

Build the compiler and server from the compiler checkout:

```powershell
cargo build --bins
```

Set `crust.server.path` to the executable, or put `crust-lsp` on `PATH`.
Open a `.cr` file to start the server. The **Crust** output channel contains
server logs. **Crust: Restart Language Server** restarts the connection.

## Build and run

Open a workspace folder, then use **Crust: Build Project**, **Crust: Check Project**, or **Crust: Run Project**
from the command palette. Commands use the project containing the active file,
or offer projects from the workspace. Run offers a choice of executable targets.

**Terminal > Run Task** lists discovered projects and their executable targets.
Compiler errors appear in the Problems panel with links to the source.

Set `crust.compiler.path` to the compiler executable. By default, the extension
looks beside an explicitly configured server, then uses `crust` on `PATH`.
Development sessions also use the local debug compiler.

To make a target the default build, add `.vscode/tasks.json` to its project:

```json
{
    "version": "2.0.0",
    "tasks": [{
        "type": "crust",
        "command": "build",
        "target": "app",
        "label": "Build app",
        "group": { "kind": "build", "isDefault": true }
    }]
}
```

The optional `project` property selects a manifest relative to the workspace
folder and defaults to `crust.toml`. Omitting `target` builds or checks all targets.

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

Run `npm test` after building the compiler to check project discovery, task
resolution, cancellation, and diagnostic links.
