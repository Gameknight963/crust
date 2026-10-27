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
    foreach ($crustRequired in @('Crust.VisualStudio.dll', 'Server/crust-lsp.exe', 'Grammars/crust.tmLanguage.json', 'language-configuration.json', 'crust.pkgdef')) {
        if (-not $crustArchive.GetEntry($crustRequired)) { throw "VSIX is missing $crustRequired" }
    }
    if ($crustArchive.Entries | Where-Object { $_.Name -like 'Microsoft.VisualStudio.*.dll' }) {
        throw 'VSIX should use the Visual Studio host assemblies.'
    }
} finally { $crustArchive.Dispose() }
[IO.Compression.ZipFile]::ExtractToDirectory($crustPackage, $crustTestRoot)
$crustSources = @{
    'Grammars/crust.tmLanguage.json' = 'editors/vscode/syntaxes/crust.tmLanguage.json'
    'language-configuration.json' = 'editors/vscode/language-configuration.json'
    'Server/crust-lsp.exe' = 'target/release/crust-lsp.exe'
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
