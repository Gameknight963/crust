param([string]$SdkRoot, [string]$CompilerPath, [string]$MSBuild)
$ErrorActionPreference = 'Stop'
$crustRepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if (-not $SdkRoot) { $SdkRoot = $PSScriptRoot }
if (-not $CompilerPath) { $CompilerPath = Join-Path $crustRepoRoot 'target/debug/crust.exe' }
if (-not $MSBuild) {
    $crustVsWhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $MSBuild = & $crustVsWhere -latest -requires Microsoft.Component.MSBuild -find 'MSBuild\**\Bin\MSBuild.exe' | Select-Object -First 1
}
if (-not $MSBuild) { throw 'Visual Studio MSBuild is required for the framework-host checks.' }
$crustSdk = [IO.Path]::GetFullPath($SdkRoot)
$crustRoot = Join-Path $crustRepoRoot ('target/msbuild-tests/' + [Guid]::NewGuid().ToString('N'))
$crustUtf8 = [Text.UTF8Encoding]::new($false)

function Write-CrustFixture([string]$Path, [string]$Text) {
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($Path)) | Out-Null
    [IO.File]::WriteAllText($Path, $Text, $crustUtf8)
}

function Escape-CrustProperty([string]$Value) {
    return [regex]::Replace($Value, "[%*`$@();?']", { param($match) '%' + ([int][char]$match.Value).ToString('X2') })
}

function Invoke-CrustBuild([string]$HostKind, [string]$Project, [string[]]$Extra, [string]$CCOverride) {
    $crustStart = New-Object Diagnostics.ProcessStartInfo
    $crustStart.FileName = if ($HostKind -eq 'core') { (Get-Command dotnet).Source } else { $MSBuild }
    # Each argument is quoted for ProcessStartInfo's argv parser. No shell runs.
    $crustArguments = @()
    if ($HostKind -eq 'core') { $crustArguments += 'msbuild' }
    $crustArguments += $Project, '-v:quiet', '-nr:false', ('-p:CrustCompilerPath=' + (Escape-CrustProperty $CompilerPath))
    $crustArguments += $Extra
    $crustStart.Arguments = ($crustArguments | ForEach-Object {
        $crustArgument = [regex]::Replace($_, '(\\*)"', '$1$1\"')
        $crustArgument = [regex]::Replace($crustArgument, '(\\+)$', '$1$1')
        '"' + $crustArgument + '"'
    }) -join ' '
    $crustStart.UseShellExecute = $false
    $crustStart.CreateNoWindow = $true
    $crustStart.RedirectStandardOutput = $true
    $crustStart.RedirectStandardError = $true
    $crustStart.StandardOutputEncoding = $crustUtf8
    $crustStart.StandardErrorEncoding = $crustUtf8
    if ($CCOverride) { $crustStart.EnvironmentVariables['CC'] = $CCOverride }
    $crustProcess = [Diagnostics.Process]::Start($crustStart)
    try {
        $crustOutput = $crustProcess.StandardOutput.ReadToEndAsync()
        $crustError = $crustProcess.StandardError.ReadToEndAsync()
        if (-not $crustProcess.WaitForExit(60000)) { $crustProcess.Kill(); throw 'MSBuild test timed out.' }
        return [pscustomobject]@{ ExitCode = $crustProcess.ExitCode; Output = $crustOutput.Result; Error = $crustError.Result }
    } finally { $crustProcess.Dispose() }
}

function Assert-CrustSuccess($Result) {
    if ($Result.ExitCode -ne 0) { throw "MSBuild failed:`n$($Result.Output)`n$($Result.Error)" }
}

$crustFakeCompiler = Join-Path $crustRoot 'invalid-metadata.exe'
$crustFakeSource = Join-Path $crustRoot 'invalid-metadata.cs'
Write-CrustFixture $crustFakeSource 'class InvalidMetadata { static void Main() { System.Console.Write("invalid JSON"); } }'
$crustCsc = Join-Path (Split-Path -Parent $MSBuild) 'Roslyn/csc.exe'
& $crustCsc /nologo /warnaserror "/out:$crustFakeCompiler" $crustFakeSource
if ($LASTEXITCODE -ne 0) { throw 'Could not compile the invalid-metadata fixture.' }

foreach ($crustHostKind in @('framework', 'core')) {
    $crustDirectory = Join-Path $crustRoot "$crustHostKind project $([char]0xfc) & spaces, with; delimiters %20 and ' quotes"
    $crustProject = Join-Path $crustDirectory 'example.crproj'
    $crustSdkXml = [Security.SecurityElement]::Escape((Escape-CrustProperty $crustSdk))
    Write-CrustFixture $crustProject @"
<Project DefaultTargets="Build">
  <PropertyGroup><CrustSdkPath>$crustSdkXml</CrustSdkPath><CrustManifest>config/crust.toml</CrustManifest></PropertyGroup>
  <Import Project="`$(CrustSdkPath)/Crust.props" />
  <Import Project="`$(CrustSdkPath)/Crust.targets" />
</Project>
"@
    Write-CrustFixture ($crustProject + '.user') '<Project><PropertyGroup><CrustRunArguments>&quot;two words&quot;</CrustRunArguments><CrustRunWorkingDirectory>work</CrustRunWorkingDirectory></PropertyGroup></Project>'
    Write-CrustFixture (Join-Path $crustDirectory 'config/crust.toml') @'
[project]
name = "msbuild-test"
[[targets]]
name = "native"
kind = "shared-library"
entry = "../src/native.cr"
[targets.native]
debug = true
[[targets]]
name = "app"
kind = "executable"
entry = "../src/app.cr"
[targets.native]
link-targets = ["native"]
debug = true
[[targets]]
name = "tool"
kind = "executable"
entry = "../src/tool.cr"
'@
    Write-CrustFixture (Join-Path $crustDirectory 'src/native.cr') 'extern int answer(int value) = "crust_answer" { return value + 1; }'
    Write-CrustFixture (Join-Path $crustDirectory 'src/app.cr') 'import "value.cr"; extern int answer(int) = "crust_answer"; int main() { unsafe { print(answer(value())); } return 0; }'
    Write-CrustFixture (Join-Path $crustDirectory 'src/value.cr') 'int value() { return 41; }'
    Write-CrustFixture (Join-Path $crustDirectory 'src/tool.cr') 'int main() { print(7); return 0; }'
    $crustMetadata = Invoke-CrustBuild $crustHostKind $crustProject @('-t:GetTargetPath', '-getTargetResult:GetTargetPath', '-getProperty:DebuggerFlavor,CrustRunArguments,CrustRunWorkingDirectory') 'missing-cc-for-check'
    Assert-CrustSuccess $crustMetadata
    $crustResult = ConvertFrom-Json $crustMetadata.Output
    $crustItems = @($crustResult.TargetResults.GetTargetPath.Items)
    if ($crustResult.Properties.DebuggerFlavor -ne 'CrustLocal' -or $crustResult.Properties.CrustRunArguments -ne '"two words"' -or $crustResult.Properties.CrustRunWorkingDirectory -ne 'work') { throw 'MSBuild lost debugger selection or local launch properties.' }
    if ($crustItems.Count -ne 3 -or ($crustItems.TargetName -join ',') -ne 'native,app,tool') { throw 'MSBuild metadata lost manifest order or targets.' }
    foreach ($crustItem in $crustItems) {
        if (-not $crustItem.Identity.StartsWith($crustDirectory + '\') -or $crustItem.Identity -ne $crustItem.TargetPath) { throw 'MSBuild changed an output path.' }
    }
    Assert-CrustSuccess (Invoke-CrustBuild $crustHostKind $crustProject @('-t:Check') 'missing-cc-for-check')
    $crustLocalCompiler = Join-Path $crustDirectory 'tools/crust.exe'
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($crustLocalCompiler)) | Out-Null
    Copy-Item -LiteralPath $CompilerPath -Destination $crustLocalCompiler
    Assert-CrustSuccess (Invoke-CrustBuild $crustHostKind $crustProject @('-t:Check', '-p:CrustCompilerPath=tools/crust.exe') 'missing-cc-for-check')
    if (Test-Path -LiteralPath (Join-Path $crustDirectory 'config/target')) { throw 'Metadata or Check built native artifacts.' }
    Assert-CrustSuccess (Invoke-CrustBuild $crustHostKind $crustProject @('-t:Build', '-p:CrustTarget=app') '')
    $crustApp = Join-Path $crustDirectory 'config/target/crust/app/app.exe'
    $crustTool = Join-Path $crustDirectory 'config/target/crust/tool/tool.exe'
    if (Test-Path -LiteralPath $crustTool) { throw 'Selecting app also built the unrelated tool target.' }
    if (-not (Test-Path -LiteralPath ([IO.Path]::ChangeExtension($crustApp, '.pdb')))) { throw 'MSBuild did not preserve the target debug setting.' }
    if ((& $crustApp) -ne '42' -or $LASTEXITCODE -ne 0) { throw 'MSBuild shared-library application did not run.' }
    Assert-CrustSuccess (Invoke-CrustBuild $crustHostKind $crustProject @('-t:Rebuild') '')
    if ((& $crustTool) -ne '7' -or $LASTEXITCODE -ne 0) { throw 'MSBuild all-target build did not produce the tool.' }
    Write-CrustFixture (Join-Path $crustDirectory 'src/value.cr') "// dependency diagnostic`nint value() { return missing; }"
    $crustInvalid = Invoke-CrustBuild $crustHostKind $crustProject @('-t:Check', '-p:CrustTarget=app') 'missing-cc-for-check'
    if ($crustInvalid.ExitCode -eq 0 -or -not $crustInvalid.Output.Contains('value.cr(2,') -or -not $crustInvalid.Output.Contains('error CRUST:')) { throw "Crust diagnostics were not reported as MSBuild errors:`n$($crustInvalid.Output)" }
    Assert-CrustSuccess (Invoke-CrustBuild $crustHostKind $crustProject @('-t:GetTargetPath') 'missing-cc-for-check')
    $crustUnknown = Invoke-CrustBuild $crustHostKind $crustProject @('-t:Build', '-p:CrustTarget=unknown') ''
    if ($crustUnknown.ExitCode -eq 0 -or -not $crustUnknown.Output.Contains("Unknown Crust target 'unknown'")) { throw 'MSBuild accepted an unknown target.' }
    $crustInvalidMetadata = Invoke-CrustBuild $crustHostKind $crustProject @('-t:GetTargetPath', ('-p:CrustCompilerPath=' + (Escape-CrustProperty $crustFakeCompiler))) ''
    if ($crustInvalidMetadata.ExitCode -eq 0 -or -not $crustInvalidMetadata.Output.Contains('invalid project metadata')) { throw 'MSBuild did not report malformed compiler metadata.' }
    Write-Output "$crustHostKind MSBuild metadata, checks, builds, native execution, target selection, and diagnostics passed"
}
