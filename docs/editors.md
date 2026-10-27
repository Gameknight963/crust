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
- Workspace symbol search across project targets and imported sources
- Rename for locals, parameters, generic type parameters, functions, methods, fields, typedefs, interfaces, source types, enum members, and external variables, with checks for name capture
- Completion for visible locals, parameters, generic parameters, imported declarations, types, keywords, and built-in functions
- Member completion for fields, instance methods, static methods, and enum values
- Signature help for functions, methods, callbacks, and built-in calls
- Semantic highlighting for resolved names, declarations, and built-in types and functions

Semantic requests use the current unsaved text and checked compiler analysis.
Workspace roots supply the importing target's context for shared files outside
the project directory, even when the entry file is closed. Adding or removing
a workspace folder rechecks diagnostics and refreshes semantic highlighting.
Clients supporting dynamic file-watcher registration watch `*.cr` and
`crust.toml` files. Disk edits, creation, and deletion recheck diagnostics and
refresh semantic highlighting, using editor text for open files. Direct clients
can also send `workspace/didChangeWatchedFiles` notifications.
Project configuration errors appear on `crust.toml` and clear when repaired.
TOML errors underline the offending key or value using UTF-16 editor positions.
Static-library targets use library analysis, including entries without `main`.
Reference searches include reads, writes, and calls. Compatible repeated
declarations share a binding, and generic instantiations share their source locations.
Rename rechecks the edited source and preserves existing identifier bindings.
Project declaration rename includes unopened targets, including interface constraints.
Native C names and enum tags are preserved through explicit aliases. Every affected target
must check successfully, and unresolved uses in unused generic bodies can prevent a rename.
Struct rename updates forward declarations and destructor names. `Self` keeps its
implicit meaning. Field rename follows designated initializers and generic
instances. Opaque C tags and native enum constants retain their native spelling.
Clients supporting versioned edits receive the current open-document version.

Completion respects lexical scopes, shadowing, and declaration order. Suggestions
replace the full identifier under the cursor. For a partial word, the server tries
a temporary repair of that word to obtain scope information. Keywords and built-in
names remain available when the program has other errors.

Member suggestions follow the receiver's resolved type, including typedefs and
generic instances. `::` offers enum values and static methods. `.` and `->` offer
fields and instance methods, with private fields visible inside their struct's
methods. Completion can use type information gathered before an unfinished
member produces a body error.

Signature help highlights the current parameter using parsed argument boundaries.
It follows nested calls and concrete generic instances, and keeps the receiver
out of an instance method's argument list. Callback signatures retain their
calling convention. Variadic arguments highlight `...`.
Partial argument lists use the same temporary analysis repair as completion.

Workspace search loads `crust.toml` targets from the editor's workspace folders,
including unopened files. It also searches open source files and their projects.
Queries match names and enclosing type names without regard to ASCII case.
Results include types, functions, methods, fields, enum values, aliases, and
external variables. Shared imports and repeated declarations appear once.
Search uses checked targets and current unsaved text, and follows workspace-folder
additions and removals.

Semantic highlighting distinguishes types, fields, methods, parameters, and other
bindings through compiler resolution. Declarations and built-in names have
separate modifiers. Token ranges use UTF-16 positions and update with unsaved
text. Clients that support semantic-token refresh requests are refreshed after
document changes, including dependency edits. A compiler error clears semantic
tokens until the program checks successfully again.

## Visual Studio

The [Visual Studio extension](../editors/visualstudio/) bundles the compiler and
`crust-lsp`.

```powershell
./editors/visualstudio/build.ps1
```

Install `target/crust-visualstudio.vsix`, restart Visual Studio, and open a folder
containing `crust.toml` and `.cr` files. The extension targets versions 17.14
through 18.x on Windows x64.

Open a source file or `crust.toml`, then use **Tools > Crust** to build, check,
or run its project. Commands save open documents, write compiler output to the
**Crust** Output pane, and put source errors in the Error List. Run selects an
executable target and opens a console with input support. The console waits for
a key after the program exits.

The client uses Visual Studio's editing UI for compiler diagnostics, completion,
navigation, references, rename, and semantic highlighting. Syntax highlighting
and bracket configuration use the same files as the VS Code extension.

Set `CRUST_LSP_PATH` to a development server's full path before starting Visual
Studio to use that server. The default is the server bundled with the VSIX.
`CRUST_COMPILER_PATH` selects a development compiler. Otherwise, commands use a
compiler beside the configured server when present, then the bundled compiler.

## VS Code

The extension lives in [editors/vscode](../editors/vscode/).

- Build and install the VSIX using the extension's README
- Set `crust.server.path` to the server executable, or put it on `PATH`
- Open a `.cr` file to get compiler diagnostics and syntax highlighting
- Use **Crust: Build Project**, **Crust: Check Project**, and **Crust: Run Project** for the active file's project
- Select discovered projects and executable targets through **Terminal > Run Task**
- Use **Crust: Restart Language Server** to restart the server

Build commands use `crust.compiler.path`, a compiler beside the explicitly
configured server, or `crust` on `PATH`. Compiler errors appear in the Problems
panel. The extension's README includes a default-build task example.

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

- Semantic information across module namespaces and project dependencies
- Visual Studio project-system integration and debugging
