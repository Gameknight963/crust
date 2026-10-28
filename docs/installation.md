# Installation

Extract a compiler archive and add its `bin` directory to `PATH`. It contains
`crust` and the standalone `crust-lsp` server. The archive also includes the
source library, examples, documentation, and compiler sources.

Install a C11 compiler and its development libraries. Crust defaults to Clang
on Windows and `cc` on Linux and macOS. On Windows, Clang uses the headers and
libraries supplied by Visual Studio C++ Build Tools. Set `CC` to select a
compiler executable.

From the extracted directory:

```text
crust --help
crust run examples/hello.cr
crust run examples/text.cr
crust run --project examples/file_project
```

## Editors

- VS Code: install `crust-language.vsix` using **Extensions: Install from VSIX**. Put the server on `PATH`, or set `crust.server.path` to its full executable path.
- Visual Studio: install `crust-visualstudio.vsix`. It includes the compiler and language server.
- Neovim and other LSP clients: launch `crust-lsp --stdio`. See [editor setup](editors.md).

## Building archives

Building an archive uses Rust, Cargo, Git, Python 3.12 or later, and a native C
compiler. Run this from the checkout:

```text
python tools/package.py --offline
```

Omit `--offline` to let Cargo fetch dependencies. Archives go to `target/packages/`,
named with the compiler version and host target. Windows uses ZIP, and Linux
and macOS use gzip-compressed tar archives.

The packager extracts the archive, verifies every file's SHA-256 digest, starts
the packaged language server, and compiles and runs the text and native-file
examples. It writes the completed archive and a `.sha256` file after these checks
pass. `manifest.json` records the version, target, source revision, working-tree
state, and file digests.
