# Crust for Visual Studio

Language support through the bundled `crust-lsp` server:

- Compiler diagnostics and semantic highlighting
- Completion, hover, and signature help
- Navigation, references, and rename
- Shared syntax highlighting and bracket configuration
- Project build and check commands with Error List navigation
- Executable target selection and console execution
- MSBuild `.crproj` projects with source files and build properties
- Console application, static library, and shared library project templates
- Native debugger launch through F5 and console execution through Ctrl+F5

## Build and install

Build on Windows x64 with Rust, the .NET 8 SDK or newer, and Visual Studio MSBuild:

```powershell
./editors/visualstudio/build.ps1
```

Install `target/crust-visualstudio.vsix` and restart Visual Studio. The extension
targets Visual Studio versions 17.14 through 18.x.

## Create a project

Choose **File > New > Project** and search for **Crust**:

- **Crust Console Application** creates an executable with a greeting
- **Crust Static Library** creates a native archive with an exported `crust_add` function
- **Crust Shared Library** creates a DLL with the same exported function

Each template creates a `.crproj`, `crust.toml`, and source file under `src/`.
Native debug information is enabled. Set a breakpoint in `src/main.cr` and press
**F5** to build and test source debugging. **Ctrl+F5** runs it
without debugging. Library projects build through the solution and can be linked
by another project's manifest.

The manifest and sources also build with the standalone compiler:

```text
crust check
crust build
crust run
```

Run these from the directory containing `crust.toml`. `crust run` selects the
console template's executable. The library templates use `crust build` and are
linked by a consuming project. The compiler reads the manifest directly.
The `.crproj` and bundled MSBuild SDK provide Visual Studio solution integration.

Install a C11 compiler and development libraries for native builds, as described
in the project README. Templates use the compiler and SDK bundled with the VSIX.

You can also open a folder containing an existing `crust.toml` and `.cr` files.

Open a `.crproj` as a project or add it to an existing solution for Solution
Explorer and solution builds. See [MSBuild projects](../msbuild/) for the project
file and command-line usage. The extension supplies its bundled SDK path.

Use `-Offline` when the Cargo and NuGet dependencies are cached. A development
server can be selected by setting `CRUST_LSP_PATH` to its full path before
starting Visual Studio.

## Build and run projects

Open a source file or `crust.toml`, then use **Tools > Crust**:

- **Build Project** builds all targets
- **Check Project** checks all targets
- **Run Project** builds an executable target and opens its console

Commands save open documents and use the project containing the active file.
Run offers a target selector when a project has multiple executables.
The **Crust** Output pane contains compiler output. Source errors also appear
in the Error List, where double-click opens the source location.

Programs inherit console input and output. The console stays open after the
program exits and closes when you press a key.

The VSIX bundles the compiler and language server. Install a C11 compiler and
development libraries for native builds, as described in the project README.
Set `CRUST_COMPILER_PATH` to a development compiler's full path before starting
Visual Studio to override the bundled compiler. Setting `CRUST_LSP_PATH` also
selects a compiler beside that server when one exists.

## Debug projects

Open a `.crproj` and set it as the solution's startup project:

- **F5** builds and starts its executable with Visual Studio's native debugger
- **Ctrl+F5** builds and starts it without debugging
- **Properties > General > Target** selects the executable when the manifest has multiple executables
- The **Debug** settings contain command arguments and a working-directory override

With an empty Target setting, launch uses the manifest's sole executable.
The default working directory is the manifest directory. Local debug settings
are saved in `.crproj.user`.

Enable debug information in the target's `crust.toml` configuration:

```toml
[targets.native]
debug = true
```

Native debugger symbols retain source names for locals, parameters, fields,
and ordinary struct types. Generic types have readable names with compiler IDs.
See [native debug information](../../docs/projects.md#native-debug-information)
for shadowing and C name collisions.

## Highlighting

The language server resolves types, fields, methods, parameters, and locals
through compiler analysis. It preserves classifications for unchanged tokens
during incomplete edits and classifies the current text again after recovery.
TextMate supplies lexical highlighting for comments, strings, keywords, and
numbers. Both layers use Visual Studio classifications and follow its editor
theme.

Preprocessor keywords, native region markers, and include filenames have distinct
classifications. Directive completion and hover help work during compiler errors.
Type `#pragma b` and complete it to get `#pragma crust begin_c`. Inside an include,
completion searches local, project, and C compiler system header directories.
See [mixed C and Crust](../../docs/mixed-source.md#standard-c-headers) for a
working `stdio.h` example.

## Development checks

```powershell
./editors/visualstudio/build.ps1 -Offline
./editors/visualstudio/test.ps1
./editors/visualstudio/test-live.ps1
```

The test checks the VSIX contents and runs the packaged client with Visual
Studio's runtime assemblies. It verifies activation, unsaved diagnostics,
recovery after an edit, and server shutdown. It also checks command resources,
project metadata, compiler diagnostics, native builds, and console input through
the packaged tools using a project path containing Unicode, spaces, and `&`.
It also tests the packaged MSBuild SDK with both MSBuild hosts, shared libraries,
target selection, relative paths, and source diagnostics.
Launch checks verify target selection, the native debug engine, F5 and Ctrl+F5
flags, arguments, and working-directory settings.
Template checks read the packaged Visual Studio catalog, create each project in
a path containing spaces and Unicode, build with both MSBuild hosts, run the
console program, and link and run consumers of both library templates.
They also build copies containing only the manifest and sources with the
standalone compiler. The client probe checks that the SDK path provider can
compose with the services exported by the CPS project scope.

The live test requires the Visual Studio extensibility tools. It installs a copy
of the VSIX in a uniquely named experimental profile, creates a project through
Visual Studio's template engine, and checks editor classifications during an
incomplete edit. It also checks directive and string colors with invalid source
and offers and commits a native region marker through Visual Studio's completion
service. The test waits for the deferred LSP completion session used by VS 18.
It verifies the first F5 build, a source breakpoint, local
evaluation, source stepping, and Ctrl+F5 execution. The experimental profile and
diagnostics under `target/visualstudio-live-tests/` remain available for inspection.
