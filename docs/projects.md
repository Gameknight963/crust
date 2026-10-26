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

The loader validates field names and TOML types. Configuration errors include
the manifest filename. Source diagnostics include the entry filename and source
location.

## Example

From the compiler checkout:

```powershell
cargo run -- run --project examples/project
cargo run -- check --project examples/project
```

## Planned additions

- Modules and imports to collect source dependencies from the entry file
- Path dependencies between Crust projects
- Static and shared library targets
- Native C sources, include paths, and link inputs
- Build profiles and platform-specific settings
- Workspaces containing related projects

These additions will extend the manifest schema. The current loader accepts
`[project]` with `name` and `[[targets]]` entries with `name`, `kind`, and `entry`.
