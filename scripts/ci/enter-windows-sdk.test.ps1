$ErrorActionPreference = 'Stop'

# Run the real consumer setup against a synthetic installation. The shell/module
# and command lookup are mocked; no Visual Studio installation is changed.
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('limo-msvc-test-' + [Guid]::NewGuid())
$installation = Join-Path $fixture 'Visual Studio'
$version = '14.44.35207'
$compiler = [IO.Path]::GetFullPath((Join-Path $installation "VC/Tools/MSVC/$version/bin/Hostx64/x64/cl.exe"))
$otherCompiler = [IO.Path]::GetFullPath((Join-Path $installation 'VC/Tools/MSVC/14.52.37025/bin/Hostx64/x64/cl.exe'))
$sdk = Join-Path $fixture 'sdk'
$header = Join-Path $sdk 'include/opencascade/Standard.hxx'
$setup = Join-Path $PSScriptRoot 'enter-windows-sdk.ps1'
$names = @('PATH', 'INCLUDE', 'LIB', 'LIBPATH', 'VCINSTALLDIR', 'VSINSTALLDIR', 'VCToolsInstallDir', 'VCToolsVersion', 'VSCMD_ARG_TGT_ARCH', 'OCCT_ROOT', 'GITHUB_ENV')
$original = @{}
foreach ($name in $names) { $original[$name] = [Environment]::GetEnvironmentVariable($name) }
$state = @{ imports = 0; shells = 0; cases = 0; selectedVersion = $version; selectedCompiler = $compiler }

function Import-Module {
    param($Name)
    if ($Name -ne (Join-Path $installation 'Common7/Tools/Microsoft.VisualStudio.DevShell.dll')) {
        throw 'Unexpected developer shell module'
    }
    $state.imports++
}
function Enter-VsDevShell {
    param($VsInstallPath, [switch]$SkipAutomaticLocation, $DevCmdArguments)
    if ($VsInstallPath -ne $installation -or -not $SkipAutomaticLocation -or
        $DevCmdArguments -cne "-arch=x64 -host_arch=x64 -vcvars_ver=$version") {
        throw 'Developer shell did not receive the exact SDK compiler and architectures'
    }
    $state.shells++
    foreach ($name in @('PATH', 'INCLUDE', 'LIB', 'LIBPATH', 'VCINSTALLDIR', 'VSINSTALLDIR', 'VCToolsInstallDir')) {
        [Environment]::SetEnvironmentVariable($name, "selected-$name")
    }
    $env:VCToolsVersion = $state.selectedVersion
    $env:VSCMD_ARG_TGT_ARCH = 'x64'
}
function Get-Command {
    param($Name, $ErrorAction)
    if ($Name -ne 'cl.exe') { throw 'Unexpected command lookup' }
    if ($state.selectedCompiler -eq 'missing') { throw 'No compiler on PATH' }
    [pscustomobject]@{ Source = $state.selectedCompiler }
}
function Reset-Fixture {
    foreach ($path in @($compiler, $otherCompiler, $header)) {
        [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path))
        [IO.File]::WriteAllText($path, 'fixture')
    }
    # The host's default changes independently from the shared SDK compiler.
    $default = Join-Path $installation 'VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt'
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($default))
    [IO.File]::WriteAllText($default, '14.52.37025')
    $state.imports = 0
    $state.shells = 0
    $state.selectedVersion = $version
    $state.selectedCompiler = $compiler
    $env:OCCT_ROOT = $sdk
    $env:GITHUB_ENV = Join-Path $fixture ([Guid]::NewGuid().ToString() + '.env')
}
function Require-Failure([string]$name, [string]$requested = $version, [bool]$beforeShell = $false) {
    $caught = $null
    try { & $setup -InstallationRoot $installation -Toolset $requested } catch { $caught = $_ }
    if ($null -eq $caught) { throw "$name unexpectedly accepted the compiler" }
    if (Test-Path -LiteralPath $env:GITHUB_ENV) { throw "$name exported an unqualified environment" }
    if ($beforeShell -and ($state.imports -ne 0 -or $state.shells -ne 0)) {
        throw "$name entered the developer shell before validation"
    }
    $state.cases++
}

try {
    Reset-Fixture
    & $setup -InstallationRoot $installation -Toolset $version
    if ($state.imports -ne 1 -or $state.shells -ne 1) { throw 'Expected one explicit shell selection' }
    $exported = @{}
    foreach ($line in [IO.File]::ReadAllLines($env:GITHUB_ENV)) {
        $pair = $line.Split('=', 2)
        $exported[$pair[0]] = $pair[1]
    }
    foreach ($name in @('INCLUDE', 'LIB', 'LIBPATH', 'VCINSTALLDIR', 'VSINSTALLDIR', 'VCToolsInstallDir')) {
        if ($exported[$name] -ne "selected-$name") { throw "Lost selected compiler environment: $name" }
    }
    if ($exported.PATH -ne "$sdk/bin;selected-PATH" -or $exported.VCToolsVersion -ne $version -or
        $exported.VSCMD_ARG_TGT_ARCH -ne 'x64' -or $exported.VCPKG_VISUAL_STUDIO_PATH -ne $installation) {
        throw 'Selected SDK/compiler environment was not retained for later steps'
    }
    $state.cases++

    foreach ($badVersion in @('14.52.37025', '', '14.44')) {
        Reset-Fixture
        $state.selectedVersion = $badVersion
        Require-Failure 'wrong selected compiler version'
    }
    foreach ($badCompiler in @($otherCompiler, (Join-Path $fixture 'Hostx64/arm64/cl.exe'), 'missing')) {
        Reset-Fixture
        $state.selectedCompiler = $badCompiler
        Require-Failure 'wrong compiler on PATH'
    }
    Reset-Fixture
    Remove-Item -LiteralPath $compiler
    Require-Failure 'missing requested compiler' $version $true

    Reset-Fixture
    Remove-Item -LiteralPath $header
    Require-Failure 'missing SDK headers'

    Reset-Fixture
    Require-Failure 'invalid compiler argument' '../14.52 -arch=arm64' $true

    Write-Output "$($state.cases) SDK compiler selection cases passed; no native tools were invoked"
} finally {
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name, $original[$name]) }
    if (Test-Path -LiteralPath $fixture) { Remove-Item -LiteralPath $fixture -Recurse -Force }
}
