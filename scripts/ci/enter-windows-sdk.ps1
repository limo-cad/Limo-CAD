param(
    [Parameter(Mandatory = $true)][string]$InstallationRoot,
    [Parameter(Mandatory = $true)][string]$Toolset
)
$ErrorActionPreference = 'Stop'

if ($Toolset -notmatch '^\d+\.\d+\.\d+(\.\d+)?$') { throw "Invalid SDK compiler version: $Toolset" }
$compiler = [IO.Path]::GetFullPath((Join-Path $InstallationRoot "VC/Tools/MSVC/$Toolset/bin/Hostx64/x64/cl.exe"))
if (-not (Test-Path -LiteralPath $compiler -PathType Leaf)) {
    throw "Required SDK compiler is missing: $compiler"
}

Import-Module (Join-Path $InstallationRoot 'Common7/Tools/Microsoft.VisualStudio.DevShell.dll')
Enter-VsDevShell -VsInstallPath $InstallationRoot -SkipAutomaticLocation -DevCmdArguments "-arch=x64 -host_arch=x64 -vcvars_ver=$Toolset"
# A runner's default version file does not identify the compiler actually used.
# Refuse fallback to another toolset, including a different cl.exe on PATH.
if ([string]::IsNullOrWhiteSpace($env:VCToolsVersion) -or
    $env:VCToolsVersion.TrimEnd('\') -ne $Toolset -or
    (Get-Command cl.exe -ErrorAction Stop).Source -ine $compiler) {
    throw "SDK/compiler mismatch: SDK $Toolset, selected $env:VCToolsVersion"
}
if (-not (Test-Path -LiteralPath "$env:OCCT_ROOT/include/opencascade/Standard.hxx" -PathType Leaf)) {
    throw 'SDK headers are missing'
}

$env:PATH = "$env:OCCT_ROOT/bin;$env:PATH"
# Carry the selected shell into every later Rust/C++ build and SDK probe.
foreach ($name in @('PATH', 'INCLUDE', 'LIB', 'LIBPATH', 'VCINSTALLDIR', 'VSINSTALLDIR', 'VCToolsInstallDir', 'VCToolsVersion', 'VSCMD_ARG_TGT_ARCH')) {
    "$name=$([Environment]::GetEnvironmentVariable($name))" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
}
"VCPKG_VISUAL_STUDIO_PATH=$InstallationRoot" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
