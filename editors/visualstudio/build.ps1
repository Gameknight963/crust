param(
    [ValidateSet('Debug', 'Release')][string]$Configuration = 'Release',
    [string]$MSBuild,
    [switch]$Offline
)
$ErrorActionPreference = 'Stop'
$crustRepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if (-not $MSBuild) {
    $crustVsWhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    if (Test-Path -LiteralPath $crustVsWhere) {
        $MSBuild = & $crustVsWhere -latest -requires Microsoft.Component.MSBuild -find 'MSBuild\**\Bin\MSBuild.exe' | Select-Object -First 1
    }
}
if (-not $MSBuild -or -not (Test-Path -LiteralPath $MSBuild)) {
    throw 'Visual Studio MSBuild was not found. Pass -MSBuild with its full path.'
}
$crustCargoArgs = @('build', '--release', '--bins', '--manifest-path', (Join-Path $crustRepoRoot 'Cargo.toml'))
$crustBuildArgs = @((Join-Path $PSScriptRoot 'Crust.VisualStudio.csproj'), '/restore', '/t:Rebuild', '/m', '/v:minimal', "/p:Configuration=$Configuration")
if ($Offline) {
    $crustCargoArgs += '--offline'
    $crustPackageCache = $env:NUGET_PACKAGES
    if (-not $crustPackageCache) { $crustPackageCache = Join-Path $env:USERPROFILE '.nuget/packages' }
    $crustBuildArgs += "/p:RestoreSources=$crustPackageCache", '/p:NuGetAudit=false'
}
& cargo @crustCargoArgs
if ($LASTEXITCODE -ne 0) { throw 'Language server build failed' }
$crustTaskArgs = @('build', (Join-Path $PSScriptRoot '../msbuild/Tasks/Crust.Build.Tasks.csproj'), '-c', $Configuration, '-v:minimal', '-m:1', '-nr:false', '-p:UseSharedCompilation=false')
if ($Offline) { $crustTaskArgs += '--source', $crustPackageCache, '-p:NuGetAudit=false' }
& dotnet @crustTaskArgs
if ($LASTEXITCODE -ne 0) { throw 'Crust MSBuild task build failed' }
$crustConsoleDirectory = Join-Path $crustRepoRoot 'target/visualstudio-console'
[IO.Directory]::CreateDirectory($crustConsoleDirectory) | Out-Null
$crustCsc = Join-Path (Split-Path -Parent $MSBuild) 'Roslyn/csc.exe'
& $crustCsc /nologo /warnaserror /langversion:latest /target:exe "/out:$crustConsoleDirectory/Crust.Console.exe" (Join-Path $PSScriptRoot 'ConsoleRunner.cs')
if ($LASTEXITCODE -ne 0) { throw 'Console launcher build failed' }
& $MSBuild @crustBuildArgs
if ($LASTEXITCODE -ne 0) { throw 'Visual Studio extension build failed' }
$crustArtifact = Join-Path $PSScriptRoot "bin/$Configuration/net472/Crust.VisualStudio.vsix"
if (-not (Test-Path -LiteralPath $crustArtifact)) { throw 'MSBuild did not produce the VSIX' }
$crustDestination = Join-Path $crustRepoRoot 'target/crust-visualstudio.vsix'
Copy-Item -LiteralPath $crustArtifact -Destination $crustDestination -Force
Write-Output "Built $crustDestination"
