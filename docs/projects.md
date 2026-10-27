# Projects

A `crust.toml` file defines a project and its executable or static-library targets:

```toml
[project]
name = "example"

[[targets]]
name = "example"
kind = "executable"
entry = "src/main.cr"
```

Entry paths are relative to the manifest. Each target compiles its entry file
and the files reached through imports.
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
- `run`, `emit-c`, `emit-header`, and `tokens` select the sole target automatically, or require `--target` for multi-target projects
- Single-file commands continue to accept a source path directly
- Each executable's output goes to `<project>/target/crust/<target>/`
- Static libraries use the same output directory, named `<target>.lib` on Windows and `lib<target>.a` on Unix
- Generated files are checked against the manifest and all entry files before writing
- Native source files and explicit link inputs are also protected from output collisions
- Imported source files from every target are protected before build output is written

The loader validates field names and TOML types. Configuration errors include
the manifest filename. Source diagnostics include the original filename and
source location, including errors in imported files.

Failed native compilation preserves the last working executable. A compiler
that exits successfully must produce a fresh native executable before `run`
starts it.

## Source imports

```c
import "buffer.cr";
```

Paths are relative to the importing file. Imported declarations participate in
the target's global namespace, including generic types, functions, and interfaces.
The compiler loads repeated imports once and permits cycles with forward references.

The [container project](../examples/container_project/) puts generic buffers,
heap owners, growable vectors, and their caller in separate files. Run it with:

```powershell
cargo run -- run --project examples/container_project
```

## Static libraries

```toml
[[targets]]
name = "math"
kind = "static-library"
entry = "src/library.cr"
```

Build with `crust build --target math`. A library entry contains functions and
imports, with no `main`. Named C exports form its public native API. Ordinary
Crust functions have internal linkage, so libraries can reuse helper names.

The build compiles generated C and native sources into objects, then creates a
fresh archive. Rebuilding removes obsolete members. Failed compilation or
archiving preserves the previous archive.

- `CC` selects the C compiler
- `AR` selects an ar-compatible executable, defaulting to `llvm-ar` on Windows and `ar` on Unix
- Native `sources`, `include-dirs`, and `defines` apply to object compilation
- Native `link-inputs` can include `.o` or `.obj` files, including matching basenames in different directories
- Consumers supply any further libraries needed by the archive

See [the library project](../examples/library_project/). Its C consumer links
against the built archive. For example, from PowerShell in this checkout:

```powershell
cargo run -- build --project examples/library_project
clang examples/library_project/native/consumer.c -Iexamples/library_project/include examples/library_project/target/crust/math/math.lib -o target/library-consumer.exe
./target/library-consumer.exe
```

On Unix, link `examples/library_project/target/crust/math/libmath.a` instead.

### C headers

`crust emit-header --target math` writes declarations for the target's named C
exports to stdout. It checks the source and imports through the compiler frontend.
The output includes C++ linkage guards, callback declarations, const pointer
levels, and native calling conventions.

- Integer and boolean exports use their native C representations
- Ordinary Crust enums use their integer representations
- Native enums retain their C tag identity and include their source C headers
- Opaque struct and union pointers retain their native tags
- Complete Crust aggregate and array layouts require a wrapper API using scalars or native opaque tags
- Private helpers and C imports are omitted

The header matches the compiler host's ABI. Generate it alongside the archive
when signatures change. For example:

```powershell
cargo run -- emit-header --project examples/library_project | Set-Content -Encoding utf8 target/library_public.h
```

### Linked targets

An executable or library can name other static-library targets in its native
configuration:

```toml
[[targets]]
name = "app"
kind = "executable"
entry = "src/main.cr"

[targets.native]
link-targets = ["math"]

[[targets]]
name = "math"
kind = "static-library"
entry = "src/math.cr"
```

`build`, `check`, and `run --target app` process linked targets before their
consumers. Dependencies reached through multiple paths are built once. Links
form a directed acyclic graph, and each reference names a static-library target.

Cross-target calls use matching `extern` declarations and named C exports.
Each archive contains its own objects. Executables link the transitive set of
archives with consumers before their dependencies.

Run [the linked project](../examples/linked_project/) with:

```powershell
cargo run -- run --project examples/linked_project --target app
```

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
defines = ["API_VERSION=2", "FEATURE_ENABLED"]
library-dirs = ["native/lib"]
libraries = ["helper"]
link-inputs = ["native/extra.o"]
```

- Paths are relative to `crust.toml` and are resolved before invoking the compiler
- Source and link input paths identify existing files
- Include and library paths identify existing directories
- `sources` compile as C11 translation units alongside the generated C
- `include-dirs` configure the native preprocessor's header search
- `defines` pass object-like or function-like C macro definitions through `-D`
- `library-dirs` configure the linker's library search
- `libraries` pass names through the C driver's `-l` option
- `link-inputs` pass explicit object or library files to the driver
- `link-targets` build and link named static-library targets from this project
- Each target has its own native configuration
- Macro names are validated and each name appears once per target
- `check`, `tokens`, `emit-c`, and `emit-header` validate configuration without invoking a C compiler

`extern header "path/to/native.h";` resolves relative to its Crust source file.
Build commands protect referenced native headers, imported Crust sources, and
native source and link inputs from being overwritten by generated output.

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

- Module namespaces and visibility
- Path dependencies between Crust projects
- Shared library targets
- Build profiles and platform-specific settings
- Workspaces containing related projects

These additions will extend the manifest schema.
