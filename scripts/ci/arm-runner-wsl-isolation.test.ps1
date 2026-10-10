$ErrorActionPreference = 'Stop'

# Exercise the real isolation script with registry operations replaced by an
# in-memory provider. IFEO's canonical and WOW6432Node paths share one key,
# matching modern Windows. These tests never access HKLM or launch WSL/OS input.
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
public sealed class FakeWslRegistryKey {
    public Dictionary<string, object> Values = new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase);
    public string[] SubKeys = new string[0];
    public string[] GetValueNames() { string[] names = new string[Values.Count]; Values.Keys.CopyTo(names, 0); return names; }
    public string[] GetSubKeyNames() { return SubKeys; }
}
public sealed class FakeWslUpdater {
    public int Id = 2468, Waits;
    public bool Completes = true;
    public bool WaitForExit(int timeout) {
        if (timeout < 1 || timeout > 90000) throw new Exception("WSL wait must stay bounded");
        Waits++;
        return Completes;
    }
}
public static class WslIsolationFixture {
    public static Dictionary<string, FakeWslRegistryKey> Keys = new Dictionary<string, FakeWslRegistryKey>();
    public static List<string> Writes = new List<string>();
    public static string FailWrite, FailRemove, Stub;
    public static bool StubExists = true;
    public static int Actions, Cases;
    public static List<FakeWslUpdater> Updaters = new List<FakeWslUpdater>();
    public static int CargoCalls, CargoExit;
    public static string Shell, ExpectedServer;
}
'@
[WslIsolationFixture]::Stub = Join-Path $env:WINDIR 'System32\systray.exe'
$paths = @(
    'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\wsl.exe',
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\wsl.exe'
)
$registryPath = $paths[0]
function Resolve-RegistryPath([string]$Path) {
    if ($Path -ceq $paths[1]) { return $registryPath }
    $Path
}
function Test-Path {
    param($LiteralPath, $PathType)
    if ($LiteralPath -eq [WslIsolationFixture]::Stub) { return [WslIsolationFixture]::StubExists }
    [WslIsolationFixture]::Keys.ContainsKey((Resolve-RegistryPath $LiteralPath))
}
function Get-Item {
    param($LiteralPath)
    $LiteralPath = Resolve-RegistryPath $LiteralPath
    if (-not [WslIsolationFixture]::Keys.ContainsKey($LiteralPath)) { throw 'Missing fake registry key' }
    [WslIsolationFixture]::Keys[$LiteralPath]
}
function New-Item {
    param($Path, [switch]$Force, $ItemType)
    if ($ItemType -eq 'Directory') { [void][IO.Directory]::CreateDirectory($Path); return }
    $Path = Resolve-RegistryPath $Path
    if (-not [WslIsolationFixture]::Keys.ContainsKey($Path)) { [WslIsolationFixture]::Keys[$Path] = [FakeWslRegistryKey]::new() }
    [WslIsolationFixture]::Writes.Add('create:' + $Path)
}
function New-ItemProperty {
    param($LiteralPath, $Name, $PropertyType, $Value)
    $LiteralPath = Resolve-RegistryPath $LiteralPath
    if ($Name -cne 'Debugger' -or $PropertyType -cne 'String' -or $Value -cne ('"' + [WslIsolationFixture]::Stub + '"')) {
        throw 'Unexpected registry policy'
    }
    if ($LiteralPath -eq [WslIsolationFixture]::FailWrite) { throw 'Simulated registry write failure' }
    [WslIsolationFixture]::Keys[$LiteralPath].Values[$Name] = $Value
    [WslIsolationFixture]::Writes.Add('set:' + $LiteralPath)
}
function Get-ItemPropertyValue {
    param($LiteralPath, $Name)
    $LiteralPath = Resolve-RegistryPath $LiteralPath
    if (-not [WslIsolationFixture]::Keys.ContainsKey($LiteralPath)) { throw 'Missing fake registry key' }
    if (-not [WslIsolationFixture]::Keys[$LiteralPath].Values.ContainsKey($Name)) { throw 'Missing fake registry value' }
    [WslIsolationFixture]::Keys[$LiteralPath].Values[$Name]
}
function Remove-ItemProperty {
    param($LiteralPath, $Name)
    $LiteralPath = Resolve-RegistryPath $LiteralPath
    if ($LiteralPath -eq [WslIsolationFixture]::FailRemove) { throw 'Simulated registry cleanup failure' }
    if (-not [WslIsolationFixture]::Keys.ContainsKey($LiteralPath) -or
        -not [WslIsolationFixture]::Keys[$LiteralPath].Values.Remove($Name)) { throw 'Missing fake registry value' }
    [WslIsolationFixture]::Writes.Add('remove-property:' + $LiteralPath)
}
function Remove-Item {
    param($LiteralPath)
    $LiteralPath = Resolve-RegistryPath $LiteralPath
    if (-not [WslIsolationFixture]::Keys.ContainsKey($LiteralPath)) { throw 'Missing fake registry key' }
    if ([WslIsolationFixture]::Keys[$LiteralPath].Values.Count -ne 0 -or
        [WslIsolationFixture]::Keys[$LiteralPath].SubKeys.Count -ne 0) { throw 'Refusing nonempty fake registry deletion' }
    [void][WslIsolationFixture]::Keys.Remove($LiteralPath)
    [WslIsolationFixture]::Writes.Add('remove-key:' + $LiteralPath)
}
function Get-Process {
    param($Name, $ErrorAction)
    if ($Name -cne 'wsl') { throw 'Unexpected process observation' }
    [WslIsolationFixture]::Updaters.ToArray()
}
function Reset-Registry {
    [WslIsolationFixture]::Keys.Clear()
    [WslIsolationFixture]::Writes.Clear()
    [WslIsolationFixture]::FailWrite = $null
    [WslIsolationFixture]::FailRemove = $null
    [WslIsolationFixture]::StubExists = $true
    [WslIsolationFixture]::Actions = 0
    [WslIsolationFixture]::Updaters.Clear()
}
$guardNames = @('GITHUB_ACTIONS', 'RUNNER_ENVIRONMENT', 'RUNNER_OS', 'RUNNER_ARCH', 'GITHUB_REPOSITORY_ID', 'GITHUB_RUN_ID', 'RUNNER_TEMP')
$original = @{}
foreach ($name in $guardNames) { $original[$name] = [Environment]::GetEnvironmentVariable($name) }
$evidenceRoot = Join-Path ([IO.Path]::GetTempPath()) ('limo-cad-wsl-isolation-fake-' + [Guid]::NewGuid())
$isolation = Join-Path $PSScriptRoot 'arm-runner-wsl-isolation.ps1'
function Invoke-Case([string]$name, [scriptblock]$action, [bool]$fails, [bool]$restored = $true) {
    [WslIsolationFixture]::Cases++
    $evidence = Join-Path $evidenceRoot ($name + '.json')
    $caught = $null
    try { & $isolation -EvidencePath $evidence -Action $action } catch { $caught = $_ }
    if (($null -ne $caught) -ne $fails) { throw "$name exception did not match expected result: $caught" }
    $report = Get-Content -LiteralPath $evidence -Raw | ConvertFrom-Json
    if ($report.status -ne $(if ($fails) { 'failed' } else { 'passed' }) -or $report.restored -ne $restored) {
        throw "$name omitted failure or restoration evidence"
    }
    if (-not $report.started_utc -or -not $report.finished_utc) { throw "$name omitted timing evidence" }
    if (-not $restored -and $report.error -and -not $caught.Exception.Message.Contains($report.error)) {
        throw "$name hid the original verification failure behind cleanup"
    }
    $report
}
$checkBoth = {
    [WslIsolationFixture]::Actions++
    foreach ($path in $paths) {
        if ((Get-ItemPropertyValue -LiteralPath $path -Name Debugger) -cne ('"' + [WslIsolationFixture]::Stub + '"')) {
            throw 'Both aliases of the shared key must be isolated before the input check'
        }
    }
}
try {
    $env:GITHUB_ACTIONS = 'true'
    $env:RUNNER_ENVIRONMENT = 'github-hosted'
    $env:RUNNER_OS = 'Windows'
    $env:RUNNER_ARCH = 'ARM64'
    $env:GITHUB_REPOSITORY_ID = '1313334315'
    $env:GITHUB_RUN_ID = '1234'
    $env:RUNNER_TEMP = $evidenceRoot

    Reset-Registry
    $report = Invoke-Case 'shared-key-success' $checkBoth $false
    if ([WslIsolationFixture]::Actions -ne 1 -or [WslIsolationFixture]::Keys.Count -ne 0 -or $report.keys.Count -ne 1 -or
        [WslIsolationFixture]::Writes.Count -ne 4) { throw 'Successful check must create, redirect and restore the shared key exactly once' }

    Reset-Registry
    $updater = [FakeWslUpdater]::new()
    [WslIsolationFixture]::Updaters.Add($updater)
    $report = Invoke-Case 'existing-updater' {
        if ([WslIsolationFixture]::Updaters[0].Waits -ne 1) { throw 'Existing WSL updater must finish before input' }
        & $checkBoth
    } $false
    if (-not $report.existing_wsl[0].finished -or [WslIsolationFixture]::Keys.Count -ne 0) { throw 'Existing updater completion and policy restoration must be recorded' }

    Reset-Registry
    $updater = [FakeWslUpdater]::new()
    $updater.Completes = $false
    [WslIsolationFixture]::Updaters.Add($updater)
    $null = Invoke-Case 'updater-timeout' $checkBoth $true
    if ($updater.Waits -ne 1 -or [WslIsolationFixture]::Actions -ne 0 -or [WslIsolationFixture]::Keys.Count -ne 0) { throw 'A WSL timeout must roll back without retrying or starting input' }

    Reset-Registry
    $null = Invoke-Case 'input-failure' { & $checkBoth; throw 'Simulated input verification failure' } $true
    if ([WslIsolationFixture]::Actions -ne 1 -or [WslIsolationFixture]::Keys.Count -ne 0) { throw 'Failed check must restore the shared key without retrying input' }

    Reset-Registry
    [WslIsolationFixture]::Keys[$registryPath] = [FakeWslRegistryKey]::new()
    [WslIsolationFixture]::Keys[$registryPath].Values['Unrelated'] = 'keep'
    $null = Invoke-Case 'existing-keys' $checkBoth $false
    if ([WslIsolationFixture]::Keys[$registryPath].Values.Count -ne 1 -or [WslIsolationFixture]::Keys[$registryPath].Values['Unrelated'] -cne 'keep') {
        throw 'Existing keys and unrelated values must survive cleanup'
    }

    foreach ($path in $paths) {
        Reset-Registry
        $physicalPath = Resolve-RegistryPath $path
        [WslIsolationFixture]::Keys[$physicalPath] = [FakeWslRegistryKey]::new()
        [WslIsolationFixture]::Keys[$physicalPath].Values['Debugger'] = 'existing-debugger'
        $null = Invoke-Case ('existing-debugger-' + [WslIsolationFixture]::Cases) $checkBoth $true
        if ([WslIsolationFixture]::Actions -ne 0 -or [WslIsolationFixture]::Writes.Count -ne 0 -or [WslIsolationFixture]::Keys[$physicalPath].Values['Debugger'] -cne 'existing-debugger') {
            throw 'A debugger policy visible through either alias must not be overwritten'
        }
    }

    Reset-Registry
    [WslIsolationFixture]::FailWrite = $registryPath
    $null = Invoke-Case 'partial-setup-failure' $checkBoth $true
    if ([WslIsolationFixture]::Actions -ne 0 -or [WslIsolationFixture]::Keys.Count -ne 0) { throw 'Partial setup must roll back without starting input' }

    Reset-Registry
    [WslIsolationFixture]::StubExists = $false
    $null = Invoke-Case 'missing-stub' $checkBoth $true
    if ([WslIsolationFixture]::Actions -ne 0 -or [WslIsolationFixture]::Writes.Count -ne 0) { throw 'Missing system stub must leave registry and input untouched' }

    Reset-Registry
    [WslIsolationFixture]::FailRemove = $registryPath
    $report = Invoke-Case 'cleanup-failure' $checkBoth $true $false
    if ([WslIsolationFixture]::Keys.Count -ne 1 -or $report.restore_errors.Count -ne 1) { throw 'Cleanup failures must be fatal and retain restoration evidence' }

    Reset-Registry
    [WslIsolationFixture]::FailRemove = $registryPath
    $report = Invoke-Case 'input-and-cleanup-failure' { & $checkBoth; throw 'Original input verification failure' } $true $false
    if ($report.error -cne 'Original input verification failure' -or $report.restore_errors.Count -ne 1) { throw 'Both verification and cleanup failures must be retained' }

    Reset-Registry
    $null = Invoke-Case 'replaced-policy' {
        & $checkBoth
        [WslIsolationFixture]::Keys[$registryPath].Values['Debugger'] = 'new-policy'
    } $true $false
    if ([WslIsolationFixture]::Keys.Count -ne 1 -or [WslIsolationFixture]::Keys[$registryPath].Values['Debugger'] -cne 'new-policy') {
        throw 'A changed policy must be preserved'
    }

    Reset-Registry
    [WslIsolationFixture]::Keys[$registryPath] = [FakeWslRegistryKey]::new()
    $null = Invoke-Case 'existing-empty-key' $checkBoth $false
    if ([WslIsolationFixture]::Keys.Count -ne 1 -or [WslIsolationFixture]::Keys[$registryPath].Values.Count -ne 0) { throw 'A preexisting empty key must survive cleanup' }

    foreach ($addition in @('value', 'subkey')) {
        Reset-Registry
        $null = Invoke-Case ('unrelated-' + $addition) {
            & $checkBoth
            if ($addition -eq 'value') { [WslIsolationFixture]::Keys[$registryPath].Values['Unrelated'] = 'keep' }
            else { [WslIsolationFixture]::Keys[$registryPath].SubKeys = @('Unrelated') }
        } $false
        if ([WslIsolationFixture]::Keys.Count -ne 1 -or [WslIsolationFixture]::Keys[$registryPath].Values.ContainsKey('Debugger') -or
            ($addition -eq 'value' -and [WslIsolationFixture]::Keys[$registryPath].Values['Unrelated'] -cne 'keep') -or
            ($addition -eq 'subkey' -and [WslIsolationFixture]::Keys[$registryPath].SubKeys[0] -cne 'Unrelated')) {
            throw 'Unrelated values and subkeys added during the test must survive cleanup'
        }
    }

    foreach ($guard in $guardNames | Where-Object { $_ -ne 'RUNNER_TEMP' }) {
        Reset-Registry
        $saved = [Environment]::GetEnvironmentVariable($guard)
        [Environment]::SetEnvironmentVariable($guard, 'unqualified')
        $caught = $null
        try { & $isolation -EvidencePath (Join-Path $evidenceRoot 'guard.json') -Action $checkBoth } catch { $caught = $_ }
        [Environment]::SetEnvironmentVariable($guard, $saved)
        [WslIsolationFixture]::Cases++
        if ($null -eq $caught -or [WslIsolationFixture]::Actions -ne 0 -or [WslIsolationFixture]::Writes.Count -ne 0) { throw "$guard must reject before registry or input actions" }
    }
    Reset-Registry
    $caught = $null
    try { & $isolation -EvidencePath (Join-Path ($evidenceRoot + '-outside') 'guard.json') -Action $checkBoth } catch { $caught = $_ }
    [WslIsolationFixture]::Cases++
    if ($null -eq $caught -or [WslIsolationFixture]::Actions -ne 0 -or [WslIsolationFixture]::Writes.Count -ne 0) { throw 'Evidence outside RUNNER_TEMP must reject before registry or input actions' }

    # Execute the actual packaged-check callback with an inert preparation
    # script and a cargo substitute. The substitute is a real child process
    # exit, so LASTEXITCODE and callback scope behave as they do on Windows.
    $tokens = $null
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../verify-windows-viewport.ps1'), [ref]$tokens, [ref]$errors)
    if ($errors.Count -ne 0) { throw 'Packaged viewport verifier has syntax errors' }
    $assignment = $ast.Find({ param($node)
        $node -is [Management.Automation.Language.AssignmentStatementAst] -and $node.Left.Extent.Text -ceq '$check'
    }, $true)
    if ($null -eq $assignment) { throw 'Missing packaged input callback' }
    $DiagnosticsDirectory = Join-Path $evidenceRoot 'packaged-check'
    $executable = Join-Path $evidenceRoot 'Limo-CAD.exe'
    $prepareDesktop = Join-Path $evidenceRoot 'inert-preparation.ps1'
    [IO.File]::WriteAllText($prepareDesktop, 'param($EvidencePath) [IO.File]::WriteAllText($EvidencePath, "prepared")')
    [WslIsolationFixture]::Shell = [Environment]::ProcessPath
    [WslIsolationFixture]::ExpectedServer = $executable
    function global:cargo {
        $arguments = @($args)
        $server = [Array]::IndexOf($arguments, '--server')
        if ($server -lt 0 -or $arguments[$server + 1] -cne [WslIsolationFixture]::ExpectedServer -or
            $arguments -notcontains 'native-control-harness' -or $arguments -notcontains '--desktop-input') {
            throw 'The packaged callback lost its executable or input feature arguments'
        }
        [WslIsolationFixture]::CargoCalls++
        & ([WslIsolationFixture]::Shell) -NoProfile -NonInteractive -Command ('exit ' + [WslIsolationFixture]::CargoExit)
    }
    foreach ($exit in @(0, 7)) {
        Reset-Registry
        [WslIsolationFixture]::CargoCalls = 0
        [WslIsolationFixture]::CargoExit = $exit
        $packagedCheck = & ([scriptblock]::Create($assignment.Right.Extent.Text))
        $null = Invoke-Case ('packaged-check-exit-' + $exit) { & $checkBoth; & $packagedCheck } ($exit -ne 0)
        if ([WslIsolationFixture]::CargoCalls -ne 1 -or [WslIsolationFixture]::Keys.Count -ne 0 -or
            [IO.File]::ReadAllText((Join-Path $DiagnosticsDirectory 'runner-account-dialog.json')) -cne 'prepared') {
            throw 'The packaged callback must prepare once, preserve failures and restore isolation'
        }
    }

    Write-Host "$([WslIsolationFixture]::Cases) mocked WSL isolation cases passed; no registry or OS input was used"
} finally {
    foreach ($name in $guardNames) { [Environment]::SetEnvironmentVariable($name, $original[$name]) }
    if ([IO.Directory]::Exists($evidenceRoot)) { [IO.Directory]::Delete($evidenceRoot, $true) }
}
