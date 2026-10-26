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
It checks each open document with the compiler frontend, publishes its first
diagnostic, and clears diagnostics when the document is fixed or closed.
Positions use UTF-16 code units. Document versions prevent stale updates from
replacing newer text.

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

- Shared symbols and type information for hover, completion, and navigation
- Project-wide analysis as modules and imports are added
- Visual Studio client and project-system integration
