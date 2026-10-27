# Crust MSBuild projects

A `.crproj` file connects MSBuild and Visual Studio to a `crust.toml` project.
The compiler reads targets, imports, and native dependencies from the manifest.

## Build from the checkout

Build the compiler and the MSBuild tasks with Rust and the .NET 8 SDK or newer:

```powershell
cargo build --bins
dotnet build editors/msbuild/Tasks/Crust.Build.Tasks.csproj -c Release
dotnet msbuild examples/file_project/files.crproj -t:Build
dotnet msbuild examples/file_project/files.crproj -t:Check
```

Visual Studio MSBuild can run the same commands. The Visual Studio extension
bundles the SDK and tasks. Open an existing `.crproj` through **File > Open >
Project/Solution**, or add it to a solution through **Add > Existing Project**.

## Project file

Place this beside `crust.toml`:

```xml
<Project DefaultTargets="Build">
  <PropertyGroup>
    <CrustSdkPath Condition="'$(CrustSdkPath)' == ''">$(CRUST_SDK_PATH)</CrustSdkPath>
  </PropertyGroup>
  <Import Project="$(CrustSdkPath)/Crust.props" />
  <PropertyGroup>
    <CrustTarget>app</CrustTarget>
  </PropertyGroup>
  <Import Project="$(CrustSdkPath)/Crust.targets" />
</Project>
```

Visual Studio supplies the bundled SDK path. For command-line builds, set
`CRUST_SDK_PATH` to this directory or pass `-p:CrustSdkPath=/path/to/sdk`.
The [file example](../../examples/file_project/files.crproj) also locates the
SDK relative to its checkout.

| Property | Purpose |
| --- | --- |
| `CrustManifest` | Manifest path relative to `.crproj`, defaulting to `crust.toml` |
| `CrustTarget` | Selected manifest target, or empty to build all targets |
| `CrustCompilerPath` | Compiler executable, with relative paths resolved beside `.crproj` |
| `CrustSdkPath` | Directory containing `Crust.props` and `Crust.targets` |

`Build` and `Rebuild` compile native artifacts. `Check` runs the compiler checks.
`GetTargetPath` returns the selected artifacts with `TargetName`, `TargetKind`,
and `TargetPath` metadata. Source errors retain their original file locations
in the MSBuild log and Visual Studio Error List.

Solution Explorer displays Crust sources and supporting files from the project
directory. Entry files and source imports determine what the compiler builds.
Local settings can go in the ignored `<project>.crproj.user` file.

## Development checks

```powershell
./editors/msbuild/test.ps1
```

The checks run Visual Studio MSBuild and `dotnet msbuild` against projects with
shared libraries, multiple targets, imported errors, relative paths, and Unicode
and punctuation in directory names.
