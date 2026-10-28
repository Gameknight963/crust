# Projects

A `crust.toml` file defines a project and its executable and library targets:

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

## Sharing Crust source

Reusable Crust code uses ordinary relative imports. A library can contain
several files, each importing its dependencies. Projects can keep a library in
their source tree or import it from a neighboring checkout.

```c
import "../../shared/box.cr";
import "../../shared/vector.cr";
```

Imported declarations share the target's scope. File-local `static` functions
hide implementation helpers. The loader deduplicates files by filesystem
identity, including files reached through different relative paths.

The repository's [library](../lib/) supplies generic containers and sorting.
These sources compile as part of the importing target, including the generic
instances that target uses.

An optional [`.crproj` file](../editors/msbuild/) adds MSBuild and Visual Studio
solution integration alongside the manifest.

## Local dependencies

Declare neighboring projects by path, relative to the manifest:

```toml
[dependencies]
support = { path = "../support" }

[[targets]]
name = "app"
kind = "executable"
entry = "src/main.cr"

[targets.native]
link-targets = ["support:api"]
```

`support:api` selects the `api` library target from that project. Dependencies
can declare their own path dependencies. Native libraries, search directories,
sanitizers, and runtime files propagate through the linked targets.

Source imports keep their relative paths and shared names. Import the Crust
API declarations or generic sources separately, for example
`import "../../support/src/api.cr";`.

- Default commands select the consuming project's targets
- Linked dependency targets build before their consumers
- A project reached through several paths shares one set of targets and artifacts
- `--target support:api` selects a dependency target explicitly
- Nested targets use `support:helper:api`
- Dependency artifacts go in `<consumer>/target/crust/dependencies/<dependency>/<target>/`
- Dependency checkouts supply inputs and retain their own build directories
- Project dependency cycles and native target-link cycles produce diagnostics

Builds compile selected targets and their dependencies afresh. Changes to
Crust source, C source, headers, and native link inputs take effect on the next
build. The [file example](../examples/file_project/) and
[crustwc](../examples/crustwc/) link the file wrapper from
[the library project](../lib/crust.toml).

## Commands

```text
crust build
crust check
crust run
crust run --target example
crust build --project path/to/crust.toml
crust metadata
```

Pass program arguments after `--`:

```text
crust run --target example -- first "two words"
crust run examples/arguments.cr -- hello
```

The executable receives them through `int main(int argc, char** argv)`.
`argv[0]` names the executable, and `argv[argc]` is a null pointer. Reading the
raw argument pointers requires `unsafe`.

- Commands discover `crust.toml` in the current directory or its parents
- A manifest path or project directory can also be passed as the input
- `build` and `check` process all targets by default
- `--target name` selects one target
- `run`, `emit-c`, `emit-header`, and `tokens` select the sole target automatically, or require `--target` for multi-target projects
- Single-file commands continue to accept a source path directly
- Each executable's output goes to `<project>/target/crust/<target>/`
- Static libraries use the same output directory, named `<target>.lib` on Windows and `lib<target>.a` on Unix
- Shared libraries use `<target>.dll` on Windows, `lib<target>.so` on Linux, and `lib<target>.dylib` on macOS
- Generated files are checked against the manifest and all entry files before writing
- Native source files and explicit link inputs are also protected from output collisions
- Imported source files from every target are protected before build output is written

The loader validates field names and TOML types. Configuration errors include
the manifest filename. Source diagnostics include the original filename and
source location, including errors in imported files.

Failed native compilation preserves the last working executable. A compiler
that exits successfully must produce a fresh native executable before `run`
starts it.

## Project metadata

`crust metadata` writes JSON for editor and build-tool integrations. It reads
project configuration and reports targets even when source files contain errors
or missing imports. `--target name` filters the target list.

| Field | Meaning |
| --- | --- |
| `formatVersion` | Metadata schema version, currently `1` |
| `manifest`, `directory` | Absolute project paths |
| `name` | Project name |
| `targets[].name`, `targets[].kind` | Target name and `executable`, `static-library`, or `shared-library` kind |
| `targets[].entry`, `targets[].output` | Absolute entry and native artifact paths |
| `targets[].linkTargets` | Names of native target dependencies |
| `targets[].runtimePaths` | Shared-library dependency directories for launching the target |
| `targets[].runtimeLibraries` | Runtime library files required by the target and its dependencies |
| `targets[].debug` | Whether native debug information is enabled |

Metadata lists the consuming project's targets in manifest order. An explicit
`--target` can select a dependency target. The command works before a
build and requires project configuration. `Project::metadata` exposes the same
information to Rust callers.

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
- `library-dirs` and `libraries` declare the archive's native link requirements
- Crust consumers inherit those requirements through `link-targets`, including transitive dependencies
- C consumers pass the required libraries to their native linker

See [the library project](../examples/library_project/). Its C consumer links
against the built archive. For example, from PowerShell in this checkout:

```powershell
cargo run -- build --project examples/library_project
clang examples/library_project/native/consumer.c -Iexamples/library_project/include examples/library_project/target/crust/math/math.lib -o target/library-consumer.exe
./target/library-consumer.exe
```

On Unix, link `examples/library_project/target/crust/math/libmath.a` instead.

## Shared libraries

```toml
[[targets]]
name = "math"
kind = "shared-library"
entry = "src/library.cr"
```

Shared libraries use the same source-level C exports as static libraries.
Build with `crust build --target math`. Windows builds also produce `math.lib`
for consumers to link. Unix consumers link the shared library itself.

`CC` selects the compiler. Native sources, includes, definitions, libraries, and
link inputs apply to the shared-library build. Static-library objects built by
Crust use position-independent code on Unix so they can link into shared libraries.

Failed builds preserve the previous shared library and its Windows import
library. Both outputs must be freshly produced and validated for a successful
Windows build.

Use `link-targets` to connect executables and libraries. `crust run` builds their
dependencies before launching the executable. Windows builds place dependency
DLLs beside their consumers and write an application manifest naming the DLLs.
Keep the executable, DLLs, and manifest together when moving an application.
Unix builds record dependency directories in runtime search paths, and `run`
also supplies the runtime library environment.

The [shared-library project](../examples/shared_project/) exports a function from
one target and calls it from another:

```powershell
cargo run -- run --project examples/shared_project --target app
```

## Sanitizers

Enable native instrumentation per target:

```toml
[targets.native]
sanitizers = ["address", "undefined"]
```

The driver instruments generated C and native sources. Sanitizer settings from
linked library targets propagate to their consumers. AddressSanitizer checks
memory access, and UndefinedBehaviorSanitizer checks native undefined behavior.
Detected errors terminate execution. The C toolchain must supply the requested
sanitizer runtimes. On Windows, deploy the ASan runtime DLL through
`runtime-libraries` or make its directory available on `PATH`.

## Native debug information

Enable debug information on each target whose code you want to inspect:

```toml
[targets.native]
debug = true
```

The driver passes `-g` to the C compiler for generated code and native sources.
Source line records point back to the original `.cr` files, including imports
and automatic cleanup. Native debuggers retain source names for parameters,
locals, and fields. Ordinary structs use their source tag names. Generic types
and types compiled alongside native headers use readable names containing their
source spelling and a compiler ID.

Shadowed bindings use distinct names containing the source spelling so cleanup
can reach outer variables. C keywords, header macros, reserved identifiers,
and names needed by generated C keep generated spellings. Static storage,
function symbols, pointer aliases, and temporary values also have generated
names.

- Windows builds using the MSVC ABI produce a `.pdb` beside the executable or DLL
- macOS builds produce an adjacent `.dSYM` bundle
- Other native formats retain the compiler's debug information in the artifact
- Static libraries retain debug information in their object files

Failed builds preserve the previous executable or library with its matching
PDB or debug bundle. Symbol output paths receive the same input protection as
native binaries. Metadata reports the target's `debug` setting without invoking
the native compiler.

## C headers

`crust emit-header --target math` writes declarations for the target's named C
exports to stdout. It checks the source and imports through the compiler frontend.
The output includes C++ linkage guards, callback declarations, const pointer
levels, and native calling conventions.

- Integer, floating-point, and boolean exports use their native C representations
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

## Linked targets

An executable or library can name static-library or shared-library targets in its native
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
form a directed acyclic graph, and each reference names a library target.

Cross-target calls use matching `extern` declarations and named C exports.
Each static archive contains its own objects. Executables and shared libraries
link the transitive set of native library artifacts with consumers before their
dependencies.

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
- Source, link input, and runtime library paths identify existing files
- Include and library paths identify existing directories
- `sources` compile as C11 translation units alongside the generated C
- `include-dirs` configure the native preprocessor's header search
- `defines` pass object-like or function-like C macro definitions through `-D`
- `debug` enables native debug information, defaulting to false
- `runtime-libraries` lists existing shared-library files used when running the target
- `library-dirs` configure the linker's library search
- `libraries` pass names through the C driver's `-l` option
- Linked targets contribute their library search directories and libraries in dependency order, preserving explicit repeated library names
- `link-inputs` pass explicit object or library files to the driver
- `link-targets` build and link named library targets from this project
- Each target has its own native configuration
- Macro names are validated and each name appears once per target
- `check`, `tokens`, `emit-c`, and `emit-header` validate configuration without invoking a C compiler

`extern header "path/to/native.h";` resolves relative to its Crust source file.
Build commands protect referenced native headers, imported Crust sources, and
native source, link input, and runtime library files from being overwritten by generated output.

Crust calls native functions through matching `extern` declarations. Calls use an
`unsafe` block. See [examples/native_project](../examples/native_project/) for a
program linked with a C source file and header.

### Runtime libraries

For an existing C DLL, select its import library and runtime file separately:

```toml
[targets.native]
link-inputs = ["vendor/widget.lib"]
runtime-libraries = ["vendor/widget.dll"]
```

Windows builds deploy the DLL beside the executable and list it in the
application manifest. On Unix, list a shared object such as `vendor/libwidget.so`
and link through `link-inputs` or `library-dirs` and `libraries`. Its directory
becomes a runtime search path.

Runtime files propagate through `link-targets`, including static-library
dependencies. Repeated dependencies share one runtime file and directory in
metadata. `runtime-libraries` can name the same shared object as `link-inputs`.

## Example

From the compiler checkout:

```powershell
cargo run -- run --project examples/project
cargo run -- check --project examples/project
cargo run -- run --project examples/native_project
```

## Planned additions

- Build profiles and platform-specific settings
- Workspaces containing related projects

These additions will extend the manifest schema.
