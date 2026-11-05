# Installation

Extract a compiler archive and add its `bin` directory to `PATH`. It contains
`crustc`, the `crust` project driver, and the standalone `crust-lsp` server.
The archive also includes the source library, examples, documentation, compiler
sources, and the libclang
runtime used to analyze C declarations.

Install a C11 compiler and its development libraries. Crust defaults to Clang
on Windows and `cc` on Linux and macOS. On Windows, Clang uses the headers and
libraries supplied by Visual Studio C++ Build Tools. Set `CC` to select a
compiler executable. Mixed-source builds use this compiler for preprocessing
and final compilation. The archive supplies libclang, so these builds do not
require a separate Clang installation.

From the extracted directory:

```text
crust --help
crust run examples/hello.cr
crust run examples/text.cr
crust run --project examples/file_project
```

`crust` invokes the adjacent `crustc` executable for each compilation. Keep both
executables in the same directory. `CRUSTC` can select another compiler executable.
The project driver resolves manifests, target dependencies, profiles, and runtime
arguments. The language server uses the compiler library directly.

To build a single source file with explicit options:

```text
crustc examples/hello.cr -o target/hello
crustc --check examples/hello.cr
crustc --emit-c examples/hello.cr
```

`-o` names the exact output file. On Windows, use `-o target/hello.exe`.
Without `-o`, the compiler derives the platform artifact name from the source
filename in the current directory. `--kind static-library` and
`--kind shared-library` compile library entries. `crustc --help` lists native
include, source, library, and sanitizer options.

Checkout development needs `cargo build --bins` before invoking the project
driver through `cargo run`. Cargo builds only the selected binary for `cargo run`.

## Editors

- VS Code: install `crust-language.vsix` using **Extensions: Install from VSIX**. Put the server on `PATH`, or set `crust.server.path` to its full executable path.
- Visual Studio: install `crust-visualstudio.vsix`. It includes the compiler and language server.
- Neovim and other LSP clients: launch `crust-lsp --stdio`. See [editor setup](editors.md).

## Building archives

Building an archive uses Rust, Cargo, Git, Python 3.12 or later, and a native C
compiler and libclang. On Linux, install `libclang-dev`. On Windows, the LLVM
installation provides `libclang.dll`. On macOS, Xcode supplies libclang. Set
`CRUST_LIBCLANG` to an explicit library file when needed. Run this from the checkout:

```text
python tools/package.py --offline
```

Omit `--offline` to let Cargo fetch dependencies. Archives go to `target/packages/`,
named with the compiler version and host target. Windows uses ZIP, and Linux
and macOS use gzip-compressed tar archives.

The packager extracts the archive, verifies every file's SHA-256 digest, starts
the packaged language server, and compiles and runs the text and native-file
examples, including a mixed C/Crust program using the bundled runtime. It also builds the file-statistics CLI and compares its UTF-8 counts
against the packaged README. It writes the completed archive and a `.sha256` file after these checks
pass. `manifest.json` records the version, target, source revision, working-tree
state, and file digests.

## Library imports

`import <std/vector.cr>;` loads the source library bundled with the compiler.
The compiler resolves its executable's symbolic links before finding the adjacent
library installation. Checkout builds use the checkout's `lib/` directory.
`CRUST_STDLIB` overrides the standard-library directory. Other angle-bracket
imports search the directories in `CRUST_LIBRARY_PATH`, using the platform's
PATH separator. For example, `<json/parser.cr>` searches for `json/parser.cr`
under those directories. Quoted imports remain relative to their source file.
Library imports reject parent-directory traversal.

Library references also work in manifest dependency and native input paths.
The file module's native backend can be linked without an installation-specific
path:

```toml
[dependencies]
standard = { path = "<std/crust.toml>" }

[[targets]]
name = "app"
entry = "main.cr"

[targets.native]
link-targets = ["standard:files"]
```

The source can then use `import <std/file.cr>;`.
