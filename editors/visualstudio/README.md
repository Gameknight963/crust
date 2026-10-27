# Crust for Visual Studio

Language support through the bundled `crust-lsp` server:

- Compiler diagnostics and semantic highlighting
- Completion, hover, and signature help
- Navigation, references, and rename
- Shared syntax highlighting and bracket configuration

## Build and install

Build on Windows x64 with Rust and Visual Studio MSBuild:

```powershell
./editors/visualstudio/build.ps1
```

Install `target/crust-visualstudio.vsix`, restart Visual Studio, and open a
folder containing `crust.toml` and `.cr` files. The extension targets Visual
Studio versions 17.14 through 18.x.

Use `-Offline` when the Cargo and NuGet dependencies are cached. A development
server can be selected by setting `CRUST_LSP_PATH` to its full path before
starting Visual Studio.

## Development checks

```powershell
./editors/visualstudio/build.ps1 -Offline
./editors/visualstudio/test.ps1
```

The test checks the VSIX contents and runs the packaged client with Visual
Studio's runtime assemblies. It verifies activation, unsaved diagnostics,
recovery after an edit, and server shutdown.
