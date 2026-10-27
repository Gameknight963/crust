param([string]$VisualStudioRoot)
$ErrorActionPreference = 'Stop'
$crustRepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if (-not $VisualStudioRoot) {
    $crustVsWhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $VisualStudioRoot = & $crustVsWhere -latest -property installationPath
}
if (-not $VisualStudioRoot) { throw 'Pass -VisualStudioRoot with the Visual Studio installation path.' }
$crustTestRoot = Join-Path $crustRepoRoot ("target/visualstudio-tests/" + [Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($crustTestRoot) | Out-Null
Add-Type -AssemblyName System.IO.Compression.FileSystem
$crustPackage = Join-Path $crustRepoRoot 'target/crust-visualstudio.vsix'
$crustArchive = [IO.Compression.ZipFile]::OpenRead($crustPackage)
try {
    foreach ($crustRequired in @('Crust.VisualStudio.dll', 'Crust.VisualStudio.pkgdef', 'Server/crust-lsp.exe', 'Server/crust.exe', 'Tools/Crust.Console.exe', 'Grammars/crust.tmLanguage.json', 'language-configuration.json', 'crust.pkgdef', 'Sdk/Crust.props', 'Sdk/Crust.targets', 'Sdk/tasks/net472/Crust.Build.Tasks.dll', 'Sdk/tasks/net8.0/Crust.Build.Tasks.dll')) {
        if (-not $crustArchive.GetEntry($crustRequired)) { throw "VSIX is missing $crustRequired" }
    }
    if ($crustArchive.Entries | Where-Object { $_.Name -like 'Microsoft.VisualStudio.*.dll' -or $_.Name -like 'Microsoft.Build.*.dll' }) {
        throw 'VSIX should use the Visual Studio host assemblies.'
    }
} finally { $crustArchive.Dispose() }
[IO.Compression.ZipFile]::ExtractToDirectory($crustPackage, $crustTestRoot)
$crustRegistration = Get-Content -LiteralPath (Join-Path $crustTestRoot 'Crust.VisualStudio.pkgdef') -Raw
if (-not $crustRegistration.Contains('"CodeBase"="$PackageFolder$\Crust.VisualStudio.dll"') -or
    -not $crustRegistration.Contains('Menus.ctmenu') -or
    -not $crustRegistration.Contains('{f9801e0c-892b-4886-8045-29fe53df7965}')) {
    throw 'VSIX is missing command-package registration or its assembly path.'
}
$crustProjectRegistration = Get-Content -LiteralPath (Join-Path $crustTestRoot 'crust.pkgdef') -Raw
if (-not $crustProjectRegistration.Contains('Projects\{419D914F-6B6D-4C61-8E53-AAB562A9C7D0}') -or
    -not $crustProjectRegistration.Contains('"DefaultProjectExtension"="crproj"')) { throw 'VSIX is missing the Crust project type.' }
$crustSources = @{
    'Grammars/crust.tmLanguage.json' = 'editors/vscode/syntaxes/crust.tmLanguage.json'
    'language-configuration.json' = 'editors/vscode/language-configuration.json'
    'Server/crust-lsp.exe' = 'target/release/crust-lsp.exe'
    'Server/crust.exe' = 'target/release/crust.exe'
    'Tools/Crust.Console.exe' = 'target/visualstudio-console/Crust.Console.exe'
    'Sdk/Crust.props' = 'editors/msbuild/Crust.props'
    'Sdk/Crust.targets' = 'editors/msbuild/Crust.targets'
    'Sdk/tasks/net472/Crust.Build.Tasks.dll' = 'target/msbuild/tasks/net472/Crust.Build.Tasks.dll'
    'Sdk/tasks/net8.0/Crust.Build.Tasks.dll' = 'target/msbuild/tasks/net8.0/Crust.Build.Tasks.dll'
}
foreach ($crustRule in @('ProjectItemsSchema', 'General', 'Compile', 'None', 'Folder', 'CrustLocal', 'DebuggerGeneral')) {
    $crustSources["Sdk/Rules/$crustRule.xaml"] = "editors/msbuild/Rules/$crustRule.xaml"
}
foreach ($crustEntry in $crustSources.GetEnumerator()) {
    $crustPackagedFile = Join-Path $crustTestRoot $crustEntry.Key
    $crustSourceFile = Join-Path $crustRepoRoot $crustEntry.Value
    if ((Get-FileHash -LiteralPath $crustPackagedFile).Hash -ne (Get-FileHash -LiteralPath $crustSourceFile).Hash) {
        throw "Packaged $($crustEntry.Key) differs from the current build input."
    }
}
$crustProbe = Join-Path $crustTestRoot 'ClientProbe.exe'
$crustCsc = Join-Path $VisualStudioRoot 'MSBuild/Current/Bin/Roslyn/csc.exe'
& $crustCsc /nologo /warnaserror /langversion:latest /r:System.Web.Extensions.dll "/out:$crustProbe" (Join-Path $PSScriptRoot 'tests/ClientProbe.cs')
if ($LASTEXITCODE -ne 0) { throw 'Client probe compilation failed' }
& $crustProbe $crustTestRoot $VisualStudioRoot
if ($LASTEXITCODE -ne 0) { throw 'Visual Studio client checks failed' }
& (Join-Path $PSScriptRoot '../msbuild/test.ps1') -SdkRoot (Join-Path $crustTestRoot 'Sdk') -CompilerPath (Join-Path $crustTestRoot 'Server/crust.exe') -MSBuild (Join-Path $VisualStudioRoot 'MSBuild/Current/Bin/MSBuild.exe')
