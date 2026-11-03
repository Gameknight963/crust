#requires -PSEdition Desktop
param([string]$Package = (Join-Path $PSScriptRoot '../../target/crust-visualstudio.vsix'))
$ErrorActionPreference = 'Stop'
$crustRepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$crustVsWhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$crustInstallation = & $crustVsWhere -latest -products '*' -requires Microsoft.VisualStudio.Component.CoreEditor -format json | ConvertFrom-Json | Select-Object -First 1
if (-not $crustInstallation) { throw 'A full Visual Studio installation is required.' }
$crustVs = $crustInstallation.installationPath
$crustIde = Join-Path $crustVs 'Common7/IDE/devenv.exe'
$crustCreate = Join-Path $crustVs 'VSSDK/VisualStudioIntegration/Tools/Bin/CreateExpInstance.exe'
if (-not (Test-Path -LiteralPath $crustCreate)) { throw 'Visual Studio extensibility tools are required.' }
$crustSuffix = 'CrustDebug' + [Guid]::NewGuid().ToString('N').Substring(0, 10)
$crustInstance = ([Diagnostics.FileVersionInfo]::GetVersionInfo($crustIde).FileMajorPart.ToString() + '.0_' + $crustInstallation.instanceId)
$crustProfile = Join-Path $env:LOCALAPPDATA "Microsoft/VisualStudio/$crustInstance$crustSuffix"
$crustRoot = Join-Path $crustRepoRoot "target/visualstudio-live-tests/$crustSuffix"
[IO.Directory]::CreateDirectory($crustRoot) | Out-Null
Write-Output "Live Visual Studio test: $crustRoot"
$crustClassifications = Join-Path $crustRoot 'classifications.json'
$crustPreviousClassifications = $env:CRUST_LIVE_CLASSIFICATIONS
$crustEditor = Join-Path $crustVs 'Common7/IDE/CommonExtensions/Microsoft/Editor'
$crustProbe = Join-Path $crustRoot 'Crust.EditorProbe.dll'
$crustReferences = @('CoreUtility', 'Text.Data', 'Text.Logic', 'Text.UI') | ForEach-Object { '/r:' + (Join-Path $crustEditor "Microsoft.VisualStudio.$_.dll") }
$crustReferences += '/r:' + (Join-Path ${env:ProgramFiles(x86)} 'Reference Assemblies/Microsoft/Framework/.NETFramework/v4.7.2/Facades/netstandard.dll')
& (Join-Path $crustVs 'MSBuild/Current/Bin/Roslyn/csc.exe') /nologo /warnaserror /target:library /r:System.ComponentModel.Composition.dll /r:System.Web.Extensions.dll @crustReferences "/out:$crustProbe" (Join-Path $PSScriptRoot 'tests/EditorProbe.cs')
if ($LASTEXITCODE -ne 0) { throw 'Editor probe compilation failed.' }
Add-Type -AssemblyName System.IO.Compression.FileSystem
$crustTestPackage = Join-Path $crustRoot 'Crust.LiveTests.vsix'
Copy-Item -LiteralPath $Package -Destination $crustTestPackage
$crustZip = [IO.Compression.ZipFile]::Open($crustTestPackage, 'Update')
try {
    [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($crustZip, $crustProbe, 'Crust.EditorProbe.dll') | Out-Null
    $crustEntry = $crustZip.GetEntry('extension.vsixmanifest')
    $crustReader = [IO.StreamReader]::new($crustEntry.Open())
    try { $crustXml = [xml]$crustReader.ReadToEnd() } finally { $crustReader.Dispose() }
    $crustAsset = $crustXml.CreateElement('Asset', $crustXml.DocumentElement.NamespaceURI)
    $crustAsset.SetAttribute('Type', 'Microsoft.VisualStudio.MefComponent')
    $crustAsset.SetAttribute('Path', 'Crust.EditorProbe.dll')
    $crustXml.PackageManifest.Assets.AppendChild($crustAsset) | Out-Null
    $crustEntry.Delete()
    $crustWriter = [IO.StreamWriter]::new($crustZip.CreateEntry('extension.vsixmanifest').Open())
    try { $crustXml.Save($crustWriter) } finally { $crustWriter.Dispose() }
} finally { $crustZip.Dispose() }
$crustInterop = Join-Path $crustVs 'Common7/IDE/PublicAssemblies/Microsoft.VisualStudio.Interop.dll'
[Reflection.Assembly]::LoadFrom($crustInterop) | Out-Null
Add-Type -Path (Join-Path $PSScriptRoot 'tests/LiveIde.cs') -ReferencedAssemblies $crustInterop
[CrustLiveIde]::RegisterFilter()

function Wait-CrustMode($Dte, [string]$Expected) {
    $crustDeadline = [DateTime]::UtcNow.AddSeconds(45)
    do {
        Start-Sleep -Milliseconds 200
        $crustMode = [CrustLiveIde]::Mode($Dte)
        if ($crustMode -eq $Expected) { return }
    } while ([DateTime]::UtcNow -lt $crustDeadline)
    throw "Expected $Expected, observed $crustMode. $([CrustLiveIde]::Output($Dte, 'Debug'))"
}

function Wait-CrustClassification([string]$Text, [hashtable]$Expected) {
    $crustDeadline = [DateTime]::UtcNow.AddSeconds(30)
    do {
        Start-Sleep -Milliseconds 200
        try {
            $crustSnapshot = Get-Content -LiteralPath $crustClassifications -Raw -ErrorAction Stop | ConvertFrom-Json
            if ($crustSnapshot.text -ne $Text) { continue }
            $crustMissing = @($Expected.GetEnumerator() | Where-Object {
                $crustName = $_.Key
                $crustType = $_.Value
                -not ($crustSnapshot.spans | Where-Object { $_.text -eq $crustName -and $_.types -contains $crustType })
            })
            if ($crustMissing.Count -eq 0) { return }
        } catch { }
    } while ([DateTime]::UtcNow -lt $crustDeadline)
    throw "Editor classifications did not match. Expected: $($Expected | ConvertTo-Json -Compress). Observed: $(Get-Content -LiteralPath $crustClassifications -Raw -ErrorAction SilentlyContinue)"
}

$crustProcess = $null
$crustDte = $null
try {
    $env:CRUST_LIVE_CLASSIFICATIONS = $crustClassifications
    & $crustCreate /Reset "/VSInstance=$crustInstance" "/RootSuffix=$crustSuffix"
    if ($LASTEXITCODE -ne 0) { throw 'Experimental profile creation failed.' }
    $crustInstall = Start-Process (Join-Path $crustVs 'Common7/IDE/VSIXInstaller.exe') -WindowStyle Hidden -Wait -PassThru -ArgumentList @('/quiet', "/rootSuffix:$crustSuffix", "/instanceIds:$($crustInstallation.instanceId)", ('"' + $crustTestPackage + '"'))
    if ($crustInstall.ExitCode -ne 0) { throw "Experimental VSIX installation failed: $($crustInstall.ExitCode)" }
    $crustUpdate = Start-Process $crustIde -WindowStyle Hidden -Wait -PassThru -ArgumentList @('/RootSuffix', $crustSuffix, '/UpdateConfiguration')
    if ($crustUpdate.ExitCode -ne 0) { throw 'Experimental profile configuration failed.' }
    $crustManifest = Get-ChildItem -LiteralPath (Join-Path $crustProfile 'Extensions') -Filter extension.vsixmanifest -Recurse | Where-Object {
        ([xml](Get-Content -LiteralPath $_.FullName -Raw)).PackageManifest.Metadata.Identity.Id -eq 'Crust.LanguageSupport.c72f2b62-66b2-4d43-bfed-3fda3d7891bf'
    } | Sort-Object { [version]([xml](Get-Content -LiteralPath $_.FullName -Raw)).PackageManifest.Metadata.Identity.Version } -Descending | Select-Object -First 1
    if (-not $crustManifest) { throw 'The test extension was not installed.' }
    $crustProcess = Start-Process $crustIde -WindowStyle Hidden -PassThru -ArgumentList @('/RootSuffix', $crustSuffix, '/ResetSettings', 'General', '/Embedding', '/Log', ('"' + $crustRoot + '/activity.xml"'))
    $crustDeadline = [DateTime]::UtcNow.AddSeconds(100)
    while (-not $crustDte -and [DateTime]::UtcNow -lt $crustDeadline) {
        if ($crustProcess.HasExited) { throw 'Experimental Visual Studio exited.' }
        $crustDte = [CrustLiveIde]::Find($crustProcess.Id)
        if (-not $crustDte) { Start-Sleep -Milliseconds 500 }
    }
    if (-not $crustDte) { throw 'Visual Studio automation was unavailable.' }
    $crustTemplate = Join-Path $crustManifest.DirectoryName 'ProjectTemplates/Crust/1033/Console/Console.vstemplate'
    $crustProject = [CrustLiveIde]::CreateProject($crustDte, $crustRoot, $crustTemplate)
    $crustDirectory = Split-Path -Parent $crustProject
    $crustSource = Join-Path $crustDirectory 'src/main.cr'
    if (-not (Test-Path -LiteralPath (Join-Path $crustDirectory 'crust.toml')) -or
        -not (Test-Path -LiteralPath $crustSource)) { throw 'Visual Studio omitted the template manifest or starter source.' }
    Write-Output 'PASS: real template creation, project loading, and SDK discovery'
    $crustSemanticText = "struct Counter { int value; };`nint main() {`n    Counter count = { .value = 42 };`n    print(count.value);`n    return 0;`n}`n"
    [CrustLiveIde]::Edit($crustDte, $crustSource, $crustSemanticText)
    $crustExpected = @{ Counter = 'struct name'; value = 'field name'; count = 'local name'; int = 'keyword'; main = 'method name' }
    Wait-CrustClassification $crustSemanticText $crustExpected
    $crustIncomplete = $crustSemanticText.Replace('return 0;', 'return ;')
    [CrustLiveIde]::Edit($crustDte, $crustSource, $crustIncomplete)
    Wait-CrustClassification $crustIncomplete $crustExpected
    Write-Output 'PASS: native editor semantic classifications survive an incomplete edit'
    $crustText = "#pragma crust begin_c`n#include <stdio.h>`nstatic int mark(void) { FILE* f = fopen(`"ran.txt`", `"wb`"); if (!f) return 1; fputs(`"run`", f); return fclose(f); }`n#pragma crust end_c`nint main() {`n    int answer = 42;`n    print(answer);`n    return mark();`n}`n"
    [CrustLiveIde]::Edit($crustDte, $crustSource, $crustText)
    [CrustLiveIde]::Breakpoint($crustDte, $crustSource, 7)
    [CrustLiveIde]::Start($crustDte)
    Wait-CrustMode $crustDte 'dbgBreakMode'
    if ([CrustLiveIde]::Value($crustDte, 'answer') -ne '42') { throw 'The debugger could not read the initialized local.' }
    [CrustLiveIde]::Step($crustDte)
    Wait-CrustMode $crustDte 'dbgBreakMode'
    if ([CrustLiveIde]::Line($crustDte) -ne 8) { throw 'Source stepping did not reach the return statement.' }
    [CrustLiveIde]::ClearBreakpoints($crustDte)
    [CrustLiveIde]::Go($crustDte)
    Wait-CrustMode $crustDte 'dbgDesignMode'
    Write-Output 'PASS: first F5 build, source breakpoint, local evaluation, and source stepping'
    $crustMarker = Join-Path $crustDirectory 'ran.txt'
    if (-not (Test-Path -LiteralPath $crustMarker)) { throw 'The debugged program did not finish.' }
    Remove-Item -LiteralPath $crustMarker
    [CrustLiveIde]::Run($crustDte)
    $crustDeadline = [DateTime]::UtcNow.AddSeconds(30)
    while (-not (Test-Path -LiteralPath $crustMarker) -and [DateTime]::UtcNow -lt $crustDeadline) { Start-Sleep -Milliseconds 200 }
    if (-not (Test-Path -LiteralPath $crustMarker)) { throw 'Ctrl+F5 did not run the executable.' }
    if ([CrustLiveIde]::Mode($crustDte) -ne 'dbgDesignMode') { throw 'Ctrl+F5 attached the debugger.' }
    Write-Output 'PASS: Ctrl+F5 native execution without debugging'
} finally {
    if ($crustDte) { try { [CrustLiveIde]::Quit($crustDte) } catch { Write-Warning $_ } }
    if ($crustProcess -and -not $crustProcess.WaitForExit(10000)) { Stop-Process -Id $crustProcess.Id -Force }
    [CrustLiveIde]::RestoreFilter()
    $env:CRUST_LIVE_CLASSIFICATIONS = $crustPreviousClassifications
    Write-Output "Retained live test profile and diagnostics: $crustProfile"
}
