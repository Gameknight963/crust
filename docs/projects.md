# Projects

A `crust.toml` file defines a project and its executable targets:

```toml
[project]
name = "example"

[[targets]]
name = "example"
kind = "executable"
entry = "src/main.cr"
```

Entry paths are relative to the manifest. Each target compiles its entry file.
Target names start with a letter or digit and use letters, digits, `_`, and `-`.
Names are unique ignoring ASCII case, giving projects consistent names across
Windows and Unix filesystems.

## Commands

```text
crust build
crust check
crust run
crust run --target example
crust build --project path/to/crust.toml
```

- Commands discover `crust.toml` in the current directory or its parents
- A manifest path or project directory can also be passed as the input
- `build` and `check` process all targets by default
- `--target name` selects one target
- `run`, `emit-c`, and `tokens` select the sole target automatically, or require `--target` for multi-target projects
- Single-file commands continue to accept a source path directly
- Each executable's output goes to `<project>/target/crust/<target>/`
- Generated files are checked against the manifest and all entry files before writing
- Native source files and explicit link inputs are also protected from output collisions

The loader validates field names and TOML types. Configuration errors include
the manifest filename. Source diagnostics include the entry filename and source
location.

## Native C code

Each target can compile C sources and link libraries:

```toml
[[targets]]
name = "app"
kind = "executable"
entry = "src/main.cr"

[targets.native]
sources = ["native/helper.c"]
include-dirs = ["include"]
library-dirs = ["native/lib"]
libraries = ["helper"]
link-inputs = ["native/extra.o"]
```

- Paths are relative to `crust.toml` and are resolved before invoking the compiler
- Source and link input paths identify existing files
- Include and library paths identify existing directories
- `sources` compile as C11 translation units alongside the generated C
- `include-dirs` configure the native preprocessor's header search
- `library-dirs` configure the linker's library search
- `libraries` pass names through the C driver's `-l` option
- `link-inputs` pass explicit object or library files to the driver
- Each target has its own native configuration
- `check`, `tokens`, and `emit-c` validate configuration without invoking a C compiler

Crust calls native functions through matching `extern` declarations. Calls use an
`unsafe` block. See [examples/native_project](../examples/native_project/) for a
program linked with a C source file and header.

## Example

From the compiler checkout:

```powershell
cargo run -- run --project examples/project
cargo run -- check --project examples/project
cargo run -- run --project examples/native_project
```

## Planned additions

- Modules and imports to collect source dependencies from the entry file
- Path dependencies between Crust projects
- Static and shared library targets
- Build profiles and platform-specific settings
- Workspaces containing related projects

These additions will extend the manifest schema.
