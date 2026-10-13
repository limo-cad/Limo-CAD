$ErrorActionPreference = 'Stop'


Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Text;
public sealed class FakeShellWindow {
    public string Title, ClassName = "Windows.UI.Core.CoreWindow", ProcessName, Executable;
    public bool Visible = true, ChangeOwner, RejectClose, StayVisible, RejectHide;
    public int IdentityReads;
    public long ForegroundAfterClose;
}
public static class HostedArmAccountWindow {
    public delegate bool EnumProc(IntPtr window, IntPtr unused);
    public static Dictionary<long, FakeShellWindow> Windows = new Dictionary<long, FakeShellWindow>();
    public static List<long> Closed = new List<long>();
    public static List<long> Hidden = new List<long>();
    public static long Foreground;
    public static bool EnumWindows(EnumProc callback, IntPtr unused) { return true; }
    public static IntPtr GetForegroundWindow() { return new IntPtr(Foreground); }
    public static bool IsWindowVisible(IntPtr window) {
        FakeShellWindow value;
        return Windows.TryGetValue(window.ToInt64(), out value) && value.Visible;
    }
    public static uint GetWindowThreadProcessId(IntPtr window, out uint pid) {
        FakeShellWindow value;
        long handle = window.ToInt64();
        pid = (uint)handle + 1000;
        if (Windows.TryGetValue(handle, out value) && value.ChangeOwner && ++value.IdentityReads > 1) pid += 10000;
        return 1;
    }
    public static int GetWindowText(IntPtr window, StringBuilder text, int count) {
        FakeShellWindow value;
        if (Windows.TryGetValue(window.ToInt64(), out value)) text.Append(value.Title);
        return text.Length;
    }
    public static int GetClassName(IntPtr window, StringBuilder text, int count) {
        FakeShellWindow value;
        if (Windows.TryGetValue(window.ToInt64(), out value)) text.Append(value.ClassName);
        return text.Length;
    }
    public static IntPtr SendMessageTimeout(IntPtr window, uint message, UIntPtr wParam, IntPtr lParam, uint flags, uint timeout, out UIntPtr result) {
        if (message != 0x0010 || flags != 3 || timeout != 1500 || wParam != UIntPtr.Zero || lParam != IntPtr.Zero)
            throw new Exception("Unexpected shell action");
        long handle = window.ToInt64();
        Closed.Add(handle);
        FakeShellWindow value = Windows[handle];
        result = UIntPtr.Zero;
        if (value.RejectClose) return IntPtr.Zero;
        if (!value.StayVisible) {
            value.Visible = false;
            if (Foreground == handle) Foreground = value.ForegroundAfterClose;
        }
        return new IntPtr(1);
    }
    public static bool ShowWindowAsync(IntPtr window, int command) {
        if (command != 0) throw new Exception("Unexpected shell hide action");
        Hidden.Add(window.ToInt64());
        FakeShellWindow value = Windows[window.ToInt64()];
        if (value.RejectHide) return false;
        value.Visible = false;
        if (Foreground == window.ToInt64()) Foreground = value.ForegroundAfterClose;
        return true;
    }
}
'@
function Add-Type { param($TypeDefinition) }
function Get-Process {
    param($Id, $ErrorAction)
    $value = [HostedArmAccountWindow]::Windows[[long]$Id - 1000]
    if ($null -eq $value) { throw 'Unknown fake process' }
    [pscustomobject]@{ ProcessName = $value.ProcessName; Path = $value.Executable }
}
function Reset-Windows {
    [HostedArmAccountWindow]::Windows.Clear()
    [HostedArmAccountWindow]::Closed.Clear()
    [HostedArmAccountWindow]::Hidden.Clear()
    [HostedArmAccountWindow]::Foreground = 0
}
function Add-Shell([long]$handle, [string]$kind) {
    $value = [FakeShellWindow]::new()
    if ($kind -eq 'Start') {
        $value.Title = 'Start'
        $value.ProcessName = 'StartMenuExperienceHost'
        $value.Executable = Join-Path $env:WINDIR 'SystemApps\Microsoft.Windows.StartMenuExperienceHost_cw5n1h2txyewy\StartMenuExperienceHost.exe'
    } else {
        $value.Title = 'Search'
        $value.ProcessName = 'SearchHost'
        $value.Executable = Join-Path $env:WINDIR 'SystemApps\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\SearchHost.exe'
    }
    [HostedArmAccountWindow]::Windows[$handle] = $value
    $value
}

function Add-WslTerminal([long]$handle) {
    $value = [FakeShellWindow]::new()
    $value.ClassName = 'CASCADIA_HOSTING_WINDOW_CLASS'
    $value.Title = Join-Path $env:WINDIR 'System32\wsl.exe'
    $value.ProcessName = 'WindowsTerminal'
    $value.Executable = Join-Path $env:ProgramFiles 'WindowsApps\Microsoft.WindowsTerminal_1.24.11911.0_arm64__8wekyb3d8bbwe\WindowsTerminal.exe'
    [HostedArmAccountWindow]::Windows[$handle] = $value
    $value
}

$guardNames = @('GITHUB_ACTIONS', 'RUNNER_ENVIRONMENT', 'RUNNER_OS', 'RUNNER_ARCH', 'GITHUB_REPOSITORY_ID', 'GITHUB_RUN_ID', 'RUNNER_TEMP')
$original = @{}
foreach ($name in $guardNames) { $original[$name] = [Environment]::GetEnvironmentVariable($name) }
$originalProgramFiles = [Environment]::GetEnvironmentVariable('ProgramFiles')
$script:caseCount = 0
$evidenceRoot = Join-Path ([IO.Path]::GetTempPath()) ('limo-cad-shell-preflight-fake-' + [Guid]::NewGuid())
$preflight = Join-Path $PSScriptRoot '../prepare-hosted-arm-desktop.ps1'
function Invoke-Case([string]$name, [string]$expected, [int]$closes, [long]$window = 0, [switch]$identify) {
    $script:caseCount++
    $path = Join-Path $evidenceRoot ($name + '.json')
    $caught = $null
    try { & $preflight -EvidencePath $path -Window $window -IdentifyOnly:$identify } catch { $caught = $_ }
    $report = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
    if ($report.status -ne $expected -or [HostedArmAccountWindow]::Closed.Count -ne $closes -or
        (($expected -eq 'failed') -ne ($null -ne $caught))) {
        throw "$name expected $expected/$closes, got $($report.status)/$([HostedArmAccountWindow]::Closed.Count): $caught"
    }
    if (-not $report.started_utc -or -not $report.finished_utc) { throw "$name omitted timing evidence" }
    $report
}
try {
    $env:GITHUB_ACTIONS = 'true'
    $env:RUNNER_ENVIRONMENT = 'github-hosted'
    $env:RUNNER_OS = 'Windows'
    $env:RUNNER_ARCH = 'ARM64'
    $env:GITHUB_REPOSITORY_ID = '1313334315'
    $env:GITHUB_RUN_ID = '1234'
    $env:RUNNER_TEMP = $evidenceRoot
    $env:ProgramFiles = Join-Path $evidenceRoot 'Program Files'

    foreach ($title in @('wsl', 'Terminal')) {
        foreach ($extended in @($false, $true)) {
            Reset-Windows
            $value = Add-WslTerminal 100
            if ($title -eq 'Terminal') { $value.Title = 'Terminal' }
            if ($extended) { $value.Executable = '\\?\' + $value.Executable }
            [HostedArmAccountWindow]::Foreground = 100
            $report = Invoke-Case ('terminal-hidden-' + $title + '-' + $extended) 'closed' 0
            if ($value.Visible -or [HostedArmAccountWindow]::Hidden.Count -ne 1 -or
                $report.terminal_windows.Count -ne 1 -or $report.terminal_windows[0].status -ne 'hidden' -or
                $report.terminal_windows[0].method -ne 'SW_HIDE') {
                throw 'The verified terminal must be hidden without closing a process or sending input'
            }
        }
    }

    foreach ($mutation in @('path', 'publisher', 'architecture', 'process', 'owner')) {
        Reset-Windows
        $value = Add-WslTerminal 100
        switch ($mutation) {
            'path' { $value.Executable = Join-Path $evidenceRoot 'WindowsTerminal.exe' }
            'publisher' { $value.Executable = $value.Executable.Replace('8wekyb3d8bbwe', 'untrusted') }
            'architecture' { $value.Executable = $value.Executable.Replace('_arm64__', '_x64__') }
            'process' { $value.ProcessName = 'UnrelatedProcess' }
            'owner' { $value.ChangeOwner = $true }
        }
        [HostedArmAccountWindow]::Foreground = 100
        $null = Invoke-Case ('wsl-terminal-refuses-' + $mutation) 'failed' 0
        if (-not $value.Visible -or [HostedArmAccountWindow]::Hidden.Count -ne 0) { throw 'An unqualified terminal must stay untouched' }
    }

    foreach ($mutation in @('title', 'class', 'unobserved')) {
        Reset-Windows
        $value = Add-WslTerminal 100
        [HostedArmAccountWindow]::Foreground = 100
        switch ($mutation) {
            'title' { $value.Title = 'A regular terminal' }
            'class' { $value.ClassName = 'UnrelatedClass' }
            'unobserved' { [HostedArmAccountWindow]::Foreground = 0 }
        }
        $null = Invoke-Case ('wsl-terminal-ignores-' + $mutation) 'not_present' 0
        if (-not $value.Visible -or [HostedArmAccountWindow]::Hidden.Count -ne 0) { throw 'Other or unobserved terminals must stay untouched' }
    }

    Reset-Windows
    $value = Add-WslTerminal 100
    $value.RejectHide = $true
    [HostedArmAccountWindow]::Foreground = 100
    $null = Invoke-Case 'wsl-terminal-refuses-hide' 'failed' 0
    if (-not $value.Visible -or [HostedArmAccountWindow]::Hidden.Count -ne 1) { throw 'Hide denial must fail without closing or retrying' }

    Reset-Windows
    $value = Add-WslTerminal 100
    [HostedArmAccountWindow]::Foreground = 100
    $null = Invoke-Case 'wsl-terminal-identify-only' 'not_present' 0 100 -identify
    if (-not $value.Visible -or [HostedArmAccountWindow]::Hidden.Count -ne 0) { throw 'Identity-only mode must never hide a terminal' }

    Reset-Windows
    $null = Add-WslTerminal 100
    $forged = Add-WslTerminal 200
    $forged.Executable = Join-Path $evidenceRoot 'WindowsTerminal.exe'
    [HostedArmAccountWindow]::Foreground = 200
    $null = Invoke-Case 'wsl-validate-all-before-hiding' 'failed' 0 100
    if ([HostedArmAccountWindow]::Hidden.Count -ne 0) { throw 'Every observed terminal must qualify before any hide' }

    foreach ($kind in @('Start', 'Search')) {
        Reset-Windows
        $null = Add-Shell 100 $kind
        [HostedArmAccountWindow]::Foreground = 100
        $report = Invoke-Case ($kind + '-foreground') 'closed' 1
        if ($report.shell_windows.Count -ne 1 -or $report.shell_windows[0].status -ne 'closed') { throw 'Shell closure evidence missing' }
    }
    foreach ($kind in @('Start', 'Search')) {
        Reset-Windows
        $account = [FakeShellWindow]::new()
        $account.Title = 'Microsoft account'
        $account.ProcessName = 'WWAHost'
        $account.Executable = Join-Path $env:WINDIR 'System32\WWAHost.exe'
        $account.ForegroundAfterClose = 200
        [HostedArmAccountWindow]::Windows[100] = $account
        $null = Add-Shell 200 $kind
        [HostedArmAccountWindow]::Foreground = 100
        $report = Invoke-Case ('account-reveals-' + $kind) 'closed' 2
        if ($report.windows.Count -ne 1 -or $report.shell_windows.Count -ne 1 -or
            $report.shell_windows[0].title -ne $kind -or $null -ne $report.foreground_after) {
            throw 'The newly foreground shell must be qualified and closed after the account dialog'
        }
    }
    Reset-Windows
    $account = [FakeShellWindow]::new()
    $account.Title = 'Microsoft account'
    $account.ProcessName = 'WWAHost'
    $account.Executable = Join-Path $env:WINDIR 'System32\WWAHost.exe'
    $account.ForegroundAfterClose = 200
    [HostedArmAccountWindow]::Windows[100] = $account
    $foreign = Add-Shell 200 'Search'
    $foreign.Title = 'Unrelated title'
    [HostedArmAccountWindow]::Foreground = 100
    $report = Invoke-Case 'account-reveals-foreign-window' 'closed' 1
    if (-not $foreign.Visible -or $report.foreground_after.hwnd -ne 200 -or
        $report.foreground_after.title -ne 'Unrelated title') { throw 'A revealed foreign window must be observed without closing it' }

    Reset-Windows
    $account = [FakeShellWindow]::new()
    $account.Title = 'Microsoft account'
    $account.ProcessName = 'WWAHost'
    $account.Executable = Join-Path $env:WINDIR 'System32\WWAHost.exe'
    $account.ForegroundAfterClose = 200
    [HostedArmAccountWindow]::Windows[100] = $account
    $forged = Add-Shell 200 'Search'
    $forged.Executable = Join-Path $evidenceRoot 'SearchHost.exe'
    [HostedArmAccountWindow]::Foreground = 100
    $null = Invoke-Case 'account-reveals-forged-shell' 'failed' 1
    if (-not $forged.Visible) { throw 'The newly revealed forged shell must be refused without closing it' }
    Reset-Windows
    $null = Add-Shell 100 'Start'
    $null = Add-Shell 200 'Search'
    [HostedArmAccountWindow]::Foreground = 200
    $report = Invoke-Case 'distinct-occluder-and-foreground' 'closed' 2 100
    if ($report.shell_windows.Count -ne 2) { throw 'Both observed shell windows must be recorded' }

    Reset-Windows
    $null = Add-Shell 100 'Start'
    [HostedArmAccountWindow]::Foreground = 100
    $null = Invoke-Case 'same-hwnd-once' 'closed' 1 100

    Reset-Windows
    $null = Add-Shell 100 'Start'
    $unobserved = Add-Shell 200 'Search'
    $null = Invoke-Case 'only-observed-window' 'closed' 1 100
    if (-not $unobserved.Visible) { throw 'Unobserved shell window was touched' }

    foreach ($kind in @('Start', 'Search')) {
        Reset-Windows
        $value = Add-Shell 100 $kind
        $value.Executable = Join-Path $evidenceRoot ($value.ProcessName + '.exe')
        $null = Invoke-Case ($kind + '-forged-path-refused') 'failed' 0 100
    }
    Reset-Windows
    $value = Add-Shell 100 'Search'
    $value.ProcessName = 'UnrelatedProcess'
    $null = Invoke-Case 'wrong-process-refused' 'failed' 0 100

    Reset-Windows
    $value = Add-Shell 100 'Start'
    $value.ClassName = 'UnrelatedClass'
    $null = Invoke-Case 'wrong-class-untouched' 'not_present' 0 100

    Reset-Windows
    $value = Add-Shell 100 'Start'
    $value.Title = 'Unrelated title'
    $null = Invoke-Case 'wrong-title-untouched' 'not_present' 0 100

    Reset-Windows
    $value = Add-Shell 100 'Start'
    $value.ChangeOwner = $true
    $null = Invoke-Case 'owner-change-refused' 'failed' 0 100

    Reset-Windows
    $null = Add-Shell 100 'Start'
    $null = Add-Shell 200 'Search'
    [HostedArmAccountWindow]::Windows[200].Executable = Join-Path $evidenceRoot 'SearchHost.exe'
    [HostedArmAccountWindow]::Foreground = 200
    $null = Invoke-Case 'validate-all-before-closing-any' 'failed' 0 100

    Reset-Windows
    $value = Add-Shell 100 'Start'
    $value.RejectClose = $true
    $null = Invoke-Case 'unacknowledged-close-fails' 'failed' 1 100

    Reset-Windows
    $value = Add-Shell 100 'Search'
    $value.StayVisible = $true
    $report = Invoke-Case 'persistent-shell-hidden' 'closed' 1 100
    if ($report.shell_windows[0].fallback -ne 'SW_HIDE') { throw 'Shell hide fallback evidence missing' }

    Reset-Windows
    $value = Add-Shell 100 'Search'
    $value.StayVisible = $true
    $value.RejectHide = $true
    $null = Invoke-Case 'visible-after-close-fails' 'failed' 1 100

    Reset-Windows
    $null = Add-Shell 100 'Start'
    $report = Invoke-Case 'identify-only-never-closes' 'not_present' 0 100 -identify
    if ($report.occluder.title -ne 'Start') { throw 'Identity-only evidence missing' }

    foreach ($guard in $guardNames | Where-Object { $_ -ne 'RUNNER_TEMP' }) {
        $before = [Environment]::GetEnvironmentVariable($guard)
        [Environment]::SetEnvironmentVariable($guard, 'invalid')
        $path = Join-Path $evidenceRoot ($guard + '-refused.json')
        $refused = $false
        try { & $preflight -EvidencePath $path -Window 100 } catch { $refused = $true }
        [Environment]::SetEnvironmentVariable($guard, $before)
        if (-not $refused -or (Test-Path -LiteralPath $path) -or [HostedArmAccountWindow]::Closed.Count -ne 0) {
            throw "$guard did not refuse before window actions or evidence writes"
        }
        $script:caseCount++
    }
    $escaped = Join-Path ([IO.Path]::GetTempPath()) ('limo-cad-refused-' + [Guid]::NewGuid() + '.json')
    $refused = $false
    try { & $preflight -EvidencePath $escaped -Window 100 } catch { $refused = $true }
    if (-not $refused -or (Test-Path -LiteralPath $escaped) -or [HostedArmAccountWindow]::Closed.Count -ne 0) { throw 'Evidence path escape did not fail closed' }
    $script:caseCount++
    Write-Output "PASS: hosted ARM shell preflight; $script:caseCount managed cases; no desktop APIs invoked"
} finally {
    foreach ($name in $guardNames) { [Environment]::SetEnvironmentVariable($name, $original[$name]) }
    [Environment]::SetEnvironmentVariable('ProgramFiles', $originalProgramFiles)
}
