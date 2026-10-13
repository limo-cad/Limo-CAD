param(
  [Parameter(Mandatory = $true)][string]$PackageDirectory,
  [Parameter(Mandatory = $true)][string]$DiagnosticsDirectory,
  [Parameter(Mandatory = $true)][switch]$ControlledSourceHost
)
$ErrorActionPreference = 'Stop'

if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
  throw 'Controlled source host OS input checks require a disposable GitHub-hosted desktop'
}
if (-not $ControlledSourceHost) { throw 'Use a separately built control-enabled source host, never the default release artifact' }
$executable = Join-Path $PackageDirectory 'Limo-CAD.exe'
if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) { throw "Missing package: $executable" }


$prepareDesktop = Join-Path $PSScriptRoot 'prepare-hosted-arm-desktop.ps1'
$check = {
  if ($env:RUNNER_ARCH -eq 'ARM64') {
    New-Item -ItemType Directory -Path $DiagnosticsDirectory -Force | Out-Null
    & $prepareDesktop -EvidencePath (Join-Path $DiagnosticsDirectory 'runner-account-dialog.json')
  }
  & cargo run --quiet --locked -p xtask --features native-control-harness -- test-mcp native-platform --desktop-input --server $executable --out (Join-Path $DiagnosticsDirectory 'native-platform')
  if ($LASTEXITCODE -ne 0) { throw 'Controlled source host native input verification failed' }
}.GetNewClosure()

& pwsh -NoProfile -NonInteractive -File (Join-Path $PSScriptRoot 'ci/arm-runner-wsl-isolation.test.ps1')
if ($LASTEXITCODE -ne 0) { throw 'WSL isolation regression tests failed' }
if ($env:RUNNER_ARCH -eq 'ARM64') {
  & (Join-Path $PSScriptRoot 'ci/arm-runner-wsl-isolation.ps1') -EvidencePath (Join-Path $DiagnosticsDirectory 'runner-wsl-isolation.json') -Action $check
} else {
  & $check
}
