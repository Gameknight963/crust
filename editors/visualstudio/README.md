# Crust for Visual Studio

Language support through the bundled `crust-lsp` server:

- Compiler diagnostics and semantic highlighting
- Completion, hover, and signature help
- Navigation, references, and rename
- Shared syntax highlighting and bracket configuration
- Project build and check commands with Error List navigation
- Executable target selection and console execution
- MSBuild `.crproj` projects with source files and build properties

## Build and install

Build on Windows x64 with Rust, the .NET 8 SDK or newer, and Visual Studio MSBuild:

```powershell
./editors/visualstudio/build.ps1
```

Install `target/crust-visualstudio.vsix`, restart Visual Studio, and open a
folder containing `crust.toml` and `.cr` files. The extension targets Visual
Studio versions 17.14 through 18.x.

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

## Development checks

```powershell
./editors/visualstudio/build.ps1 -Offline
./editors/visualstudio/test.ps1
```

The test checks the VSIX contents and runs the packaged client with Visual
Studio's runtime assemblies. It verifies activation, unsaved diagnostics,
recovery after an edit, and server shutdown. It also checks command resources,
project metadata, compiler diagnostics, native builds, and console input through
the packaged tools using a project path containing Unicode, spaces, and `&`.
It also tests the packaged MSBuild SDK with both MSBuild hosts, shared libraries,
target selection, relative paths, and source diagnostics.
