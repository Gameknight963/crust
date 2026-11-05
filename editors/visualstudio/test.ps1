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
Add-Type -AssemblyName System.Xaml
[Reflection.Assembly]::LoadFrom((Join-Path $VisualStudioRoot 'MSBuild/Current/Bin/Microsoft.Build.Framework.dll')) | Out-Null
Add-Type -AssemblyName System.IO.Compression.FileSystem
$crustPackage = Join-Path $crustRepoRoot 'target/crust-visualstudio.vsix'
$crustArchive = [IO.Compression.ZipFile]::OpenRead($crustPackage)
try {
    foreach ($crustRequired in @('Licenses/Gflat.txt', 'Licenses/Roslyn.txt', 'Crust.VisualStudio.dll', 'Crust.VisualStudio.pkgdef', 'Server/crust-lsp.exe', 'Server/crust.exe', 'Server/crustc.exe', 'lib/box.cr', 'lib/native/file.h', 'Server/libclang.dll', 'Server/third-party/llvm/LICENSE.TXT', 'Tools/Crust.Console.exe', 'Grammars/crust.tmLanguage.json', 'language-configuration.json', 'crust.pkgdef', 'Sdk/Crust.props', 'Sdk/Crust.targets', 'Sdk/tasks/net472/Crust.Build.Tasks.dll', 'Sdk/tasks/net8.0/Crust.Build.Tasks.dll')) {
        if (-not $crustArchive.GetEntry($crustRequired)) { throw "VSIX is missing $crustRequired" }
    }
    foreach ($crustRuleEntry in ($crustArchive.Entries | Where-Object { $_.FullName -like 'Sdk/Rules/*.xaml' })) {
        $crustRuleReader = [IO.StreamReader]::new($crustRuleEntry.Open())
        try { $crustRuleText = $crustRuleReader.ReadToEnd() } finally { $crustRuleReader.Dispose() }
        try { [System.Xaml.XamlServices]::Parse($crustRuleText) | Out-Null }
        catch { throw "Invalid packaged CPS rule $($crustRuleEntry.FullName): $($_.Exception.Message)" }
    }
    Write-Output 'Packaged CPS rules parse with the MSBuild XAML schema'
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
    -not $crustProjectRegistration.Contains('"DefaultProjectExtension"="crproj"') -or
    -not $crustProjectRegistration.Contains('"Language(VsTemplate)"="Crust"') -or
    -not $crustProjectRegistration.Contains('"TemplatesDir"="$PackageFolder$\ProjectTemplates"')) { throw 'VSIX is missing the Crust project type or template registration.' }
$crustManifest = [xml](Get-Content -LiteralPath (Join-Path $crustTestRoot 'extension.vsixmanifest') -Raw)
if (-not ($crustManifest.PackageManifest.Assets.Asset | Where-Object { $_.Type -eq 'Microsoft.VisualStudio.ProjectTemplate' -and $_.Path -eq 'ProjectTemplates' })) {
    throw 'VSIX is missing its project-template asset.'
}
$crustSources = @{
    'Grammars/crust.tmLanguage.json' = 'editors/vscode/syntaxes/crust.tmLanguage.json'
    'Grammars/crust.tmLanguage.tmTheme' = 'editors/visualstudio/crust.tmLanguage.tmTheme'
    'language-configuration.json' = 'editors/vscode/language-configuration.json'
    'Server/crust-lsp.exe' = 'target/release/crust-lsp.exe'
    'Server/crust.exe' = 'target/release/crust.exe'
    'Server/crustc.exe' = 'target/release/crustc.exe'
    'Server/libclang.dll' = 'target/native-runtime/libclang.dll'
    'Server/third-party/llvm/LICENSE.TXT' = 'third-party/llvm/LICENSE.TXT'
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
$crustEditorAssemblies = Join-Path $VisualStudioRoot 'Common7/IDE/CommonExtensions/Microsoft/Editor'
$crustThemeReferences = @('CoreUtility', 'Text.Data', 'Text.Logic', 'Text.UI', 'Text.UI.Wpf') | ForEach-Object { '/r:' + (Join-Path $crustEditorAssemblies "Microsoft.VisualStudio.$_.dll") }
$crustFrameworkReferences = Join-Path ${env:ProgramFiles(x86)} 'Reference Assemblies/Microsoft/Framework/.NETFramework/v4.7.2'
$crustThemeReferences += '/r:' + (Join-Path $crustFrameworkReferences 'PresentationCore.dll')
$crustThemeReferences += '/r:' + (Join-Path $crustFrameworkReferences 'WindowsBase.dll')
$crustThemeReferences += '/r:' + (Join-Path ${env:ProgramFiles(x86)} 'Reference Assemblies/Microsoft/Framework/.NETFramework/v4.7.2/Facades/netstandard.dll')
& (Join-Path $VisualStudioRoot 'MSBuild/Current/Bin/Roslyn/csc.exe') /nologo /warnaserror /target:library @crustThemeReferences "/out:$crustTestRoot/ThemeColorCheck.dll" (Join-Path $PSScriptRoot 'tests/ThemeColorCheck.cs')
if ($LASTEXITCODE -ne 0) { throw 'Tooltip theme test compilation failed' }
$crustProbe = Join-Path $crustTestRoot 'ClientProbe.exe'
$crustCsc = Join-Path $VisualStudioRoot 'MSBuild/Current/Bin/Roslyn/csc.exe'
& $crustCsc /nologo /warnaserror /langversion:latest /r:System.Web.Extensions.dll /r:System.ComponentModel.Composition.dll "/out:$crustProbe" (Join-Path $PSScriptRoot 'tests/ClientProbe.cs')
if ($LASTEXITCODE -ne 0) { throw 'Client probe compilation failed' }
& $crustProbe $crustTestRoot $VisualStudioRoot
if ($LASTEXITCODE -ne 0) { throw 'Visual Studio client checks failed' }
& (Join-Path $PSScriptRoot '../msbuild/test.ps1') -SdkRoot (Join-Path $crustTestRoot 'Sdk') -CompilerPath (Join-Path $crustTestRoot 'Server/crust.exe') -MSBuild (Join-Path $VisualStudioRoot 'MSBuild/Current/Bin/MSBuild.exe')
& python (Join-Path $PSScriptRoot 'tests/project_templates.py') $crustTestRoot $VisualStudioRoot
if ($LASTEXITCODE -ne 0) { throw 'Packaged project template checks failed' }

# Include native header discovery in the cold analysis, then ensure the packaged
# server reuses that snapshot for ordinary editor requests. This also runs in CI.
$crustPerformanceRoot = Join-Path $crustTestRoot 'performance'
[IO.Directory]::CreateDirectory($crustPerformanceRoot) | Out-Null
$crustPerformanceSource = Join-Path $crustPerformanceRoot 'main.cr'
[IO.File]::WriteAllText($crustPerformanceSource, "#pragma crust begin_c`n#include <stdio.h>`n#pragma crust end_c`nint answer(int value) { return value; }`nint main() { return answer(42); }`n")
& python (Join-Path $crustRepoRoot 'tools/benchmark_lsp.py') $crustPerformanceSource --server (Join-Path $crustTestRoot 'Server/crust-lsp.exe') --line 4 --character 22 --repeat 5 --require-clean --max-warm-ms 250
if ($LASTEXITCODE -ne 0) { throw 'Packaged editor latency regression failed' }
