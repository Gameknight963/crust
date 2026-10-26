# Editor support

`crust-lsp` provides compiler diagnostics over the Language Server Protocol.
Editors send document text over stdio, including unsaved changes.

## Build the server

```powershell
cargo build --bins
```

Put `target/debug/crust-lsp` on `PATH`, or configure the editor with its absolute
path. On Windows the executable is `crust-lsp.exe`.

The server starts with `crust-lsp --stdio` and uses full-document synchronization.
It checks entry files and their imports with the compiler frontend, using unsaved
text for open dependencies. In a project, opening a dependency also analyzes its
entry target. Diagnostics appear on the source file containing the error and
update when a dependency changes or an import is removed.
Positions use UTF-16 code units. Document versions prevent stale updates from
replacing newer text.

The server also provides:

- Hover for source declarations and checked expression types
- Go to definition for values, functions, members, typedefs, type names, interfaces, and generic parameters
- Source outlines for types, interfaces, typedefs, fields, functions, and methods, including unused generic declarations
- Find All References for resolved bindings across the analyzed target's imports
- Rename for locals and explicit parameters, with checks for name capture
- Completion for visible locals, parameters, generic parameters, imported declarations, types, keywords, and built-in functions

Semantic requests use the current unsaved text and checked compiler analysis.
Reference searches include reads, writes, and calls. Compatible repeated
declarations share a binding, and generic instantiations share their source locations.
Rename rechecks the edited source and preserves existing identifier bindings.
Clients supporting versioned edits receive the current open-document version.

Completion respects lexical scopes, shadowing, and declaration order. Suggestions
replace the full identifier under the cursor. For a partial word, the server tries
a temporary repair of that word to obtain scope information. Keywords and built-in
names remain available when the program has other errors.

## VS Code

The extension lives in [editors/vscode](../editors/vscode/).

- Build and install the VSIX using the extension's README
- Set `crust.server.path` to the server executable, or put it on `PATH`
- Open a `.cr` file to get compiler diagnostics and syntax highlighting
- Use **Crust: Restart Language Server** to restart the server

The compiler checkout includes a **Crust: VS Code extension** launch configuration
for extension development.

## Neovim

For Neovim 0.11 and later:

```lua
vim.filetype.add({ extension = { cr = 'crust' } })

vim.lsp.config('crust', {
    cmd = { 'crust-lsp', '--stdio' },
    filetypes = { 'crust' },
    root_markers = { 'crust.toml', '.git' },
})

vim.lsp.enable('crust')
```

This uses Neovim's built-in LSP client. See its
[LSP configuration reference](https://neovim.io/doc/user/lsp.html#lsp-config).

## Other clients

Launch `crust-lsp --stdio`, select language ID `crust`, and use the capabilities
returned by `initialize`. Protocol output goes to stdout and process errors go
to stderr.

## Further work

- Member completion and rename for additional symbol kinds through the shared symbol index
- Semantic information across module namespaces and project dependencies
- Visual Studio client and project-system integration
