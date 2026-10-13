param(
    [Parameter(Mandatory = $true)][string]$EvidencePath,
    [long]$Window = 0,
    [switch]$IdentifyOnly
)
$ErrorActionPreference = 'Stop'




if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or
    $env:RUNNER_OS -ne 'Windows' -or $env:RUNNER_ARCH -ne 'ARM64' -or
    $env:GITHUB_REPOSITORY_ID -ne '1313334315' -or $env:GITHUB_RUN_ID -notmatch '^\d+$') {
    throw 'Account-window preparation requires the disposable GitHub-hosted ARM64 package runner'
}
$runnerRoot = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
$evidenceFile = [IO.Path]::GetFullPath($EvidencePath)
if (-not $evidenceFile.StartsWith($runnerRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Runner evidence must stay inside RUNNER_TEMP'
}

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class HostedArmAccountWindow {
    public delegate bool EnumProc(IntPtr window, IntPtr unused);
    [DllImport("user32.dll", SetLastError = true)] public static extern bool EnumWindows(EnumProc callback, IntPtr unused);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll", SetLastError = true)] public static extern IntPtr SendMessageTimeout(IntPtr window, uint message, UIntPtr wParam, IntPtr lParam, uint flags, uint timeout, out UIntPtr result);
    [DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr window, int command);
}
'@

function Get-CoveringWindowIdentity([IntPtr]$window) {

    $title = [Text.StringBuilder]::new(512)
    $class = [Text.StringBuilder]::new(512)
    [void][HostedArmAccountWindow]::GetWindowText($window, $title, 512)
    [void][HostedArmAccountWindow]::GetClassName($window, $class, 512)
    [uint32]$owner = 0
    [void][HostedArmAccountWindow]::GetWindowThreadProcessId($window, [ref]$owner)
    $processName = $null
    $executable = $null
    $executableError = $null
    try {
        $process = Get-Process -Id $owner -ErrorAction Stop
        $processName = $process.ProcessName
        try { $executable = $process.Path } catch { $executableError = $_.Exception.Message }
    } catch {
        $executableError = $_.Exception.Message
    }
    $identity = [ordered]@{
        hwnd = $window.ToInt64()
        process_id = $owner
        process_name = $processName
        title = $title.ToString()
        class = $class.ToString()
        executable = $executable
    }
    if ($null -ne $executableError) { $identity.executable_error = $executableError }
    [pscustomobject]$identity
}

function Get-AccountWindow([IntPtr]$window) {
    if (-not [HostedArmAccountWindow]::IsWindowVisible($window)) { return $null }
    $title = [Text.StringBuilder]::new(512)
    $class = [Text.StringBuilder]::new(512)
    [void][HostedArmAccountWindow]::GetWindowText($window, $title, 512)
    [void][HostedArmAccountWindow]::GetClassName($window, $class, 512)
    if ($title.ToString() -cne 'Microsoft account' -or $class.ToString() -cne 'Windows.UI.Core.CoreWindow') { return $null }
    [uint32]$owner = 0
    [void][HostedArmAccountWindow]::GetWindowThreadProcessId($window, [ref]$owner)
    $process = Get-Process -Id $owner -ErrorAction Stop
    [pscustomobject][ordered]@{
        hwnd = $window.ToInt64()
        process_id = $owner
        process_name = $process.ProcessName
        executable = $process.Path
        title = $title.ToString()
        class = $class.ToString()
    }
}

function Get-ShellWindow([IntPtr]$window) {
    if (-not [HostedArmAccountWindow]::IsWindowVisible($window)) { return $null }
    $identity = Get-CoveringWindowIdentity $window
    if ($identity.class -cne 'Windows.UI.Core.CoreWindow') { return $null }
    if ($identity.title -ceq 'Start') {
        $name = 'StartMenuExperienceHost'
        $relative = 'SystemApps\Microsoft.Windows.StartMenuExperienceHost_cw5n1h2txyewy\StartMenuExperienceHost.exe'
    } elseif ($identity.title -ceq 'Search') {
        $name = 'SearchHost'
        $relative = 'SystemApps\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\SearchHost.exe'
    } else { return $null }
    $expectedExecutable = Join-Path $env:WINDIR $relative
    if ($identity.process_name -cne $name -or
        -not [string]::Equals($identity.executable, $expectedExecutable, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing a $($identity.title) window whose process is not its exact system executable"
    }
    $identity
}

function Close-ObservedShellWindows {


    $candidates = [Collections.Generic.List[object]]::new()
    $seen = [Collections.Generic.HashSet[long]]::new()
    foreach ($observed in @($Window, [HostedArmAccountWindow]::GetForegroundWindow().ToInt64())) {
        if ($observed -eq 0 -or -not $seen.Add($observed)) { continue }
        $candidate = Get-ShellWindow ([IntPtr]::new($observed))
        if ($null -ne $candidate) { $candidates.Add($candidate) }
    }
    $report.shell_windows = @($report.shell_windows) + @($candidates.ToArray())

    foreach ($candidate in $candidates) {
        $shellWindow = [IntPtr]::new($candidate.hwnd)
        if (-not [HostedArmAccountWindow]::IsWindowVisible($shellWindow)) {
            $candidate | Add-Member -NotePropertyName status -NotePropertyValue 'already_hidden'
            continue
        }
        $current = Get-ShellWindow $shellWindow
        if ($null -eq $current -or $current.process_id -ne $candidate.process_id -or
            $current.title -cne $candidate.title -or $current.process_name -cne $candidate.process_name -or
            -not [string]::Equals($current.executable, $candidate.executable, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Shell-window identity changed before runner preparation; nothing was sent'
        }
        [UIntPtr]$messageResult = [UIntPtr]::Zero
        $sent = [HostedArmAccountWindow]::SendMessageTimeout($shellWindow, 0x0010, [UIntPtr]::Zero, [IntPtr]::Zero, 3, 1500, [ref]$messageResult)
        if ($sent -eq [IntPtr]::Zero) {
            throw "Shell-window WM_CLOSE was not acknowledged (Win32 error $([Runtime.InteropServices.Marshal]::GetLastWin32Error()))"
        }
        $deadline = [DateTime]::UtcNow.AddSeconds(5)
        while ([HostedArmAccountWindow]::IsWindowVisible($shellWindow)) {
            [uint32]$owner = 0
            [void][HostedArmAccountWindow]::GetWindowThreadProcessId($shellWindow, [ref]$owner)
            if ($owner -ne $candidate.process_id) { break }
            if ([DateTime]::UtcNow -ge $deadline) {
                $current = Get-ShellWindow $shellWindow
                if ($null -eq $current -or $current.process_id -ne $candidate.process_id -or
                    $current.title -cne $candidate.title -or $current.process_name -cne $candidate.process_name -or
                    -not [string]::Equals($current.executable, $candidate.executable, [StringComparison]::OrdinalIgnoreCase)) {
                    throw 'Shell-window identity changed before the hosted-runner hide fallback'
                }
                if (-not [HostedArmAccountWindow]::ShowWindowAsync($shellWindow, 0)) {
                    throw 'The verified hosted-runner shell window refused SW_HIDE'
                }
                $hideDeadline = [DateTime]::UtcNow.AddSeconds(2)
                while ([HostedArmAccountWindow]::IsWindowVisible($shellWindow)) {
                    [uint32]$hideOwner = 0
                    [void][HostedArmAccountWindow]::GetWindowThreadProcessId($shellWindow, [ref]$hideOwner)
                    if ($hideOwner -ne $candidate.process_id) { break }
                    if ([DateTime]::UtcNow -ge $hideDeadline) { throw 'The hosted-runner shell window remained visible after SW_HIDE' }
                    Start-Sleep -Milliseconds 50
                }
                $candidate | Add-Member -NotePropertyName fallback -NotePropertyValue 'SW_HIDE'
                break
            }
            Start-Sleep -Milliseconds 50
        }
        $candidate | Add-Member -NotePropertyName status -NotePropertyValue 'closed'
    }
}

function Get-HostedTerminalWindow([IntPtr]$window) {
    if (-not [HostedArmAccountWindow]::IsWindowVisible($window)) { return $null }
    $identity = Get-CoveringWindowIdentity $window
    $expectedTitle = Join-Path $env:WINDIR 'System32\wsl.exe'
    if ($identity.class -cne 'CASCADIA_HOSTING_WINDOW_CLASS' -or
        ($identity.title -cne 'Terminal' -and
         -not [string]::Equals($identity.title, $expectedTitle, [StringComparison]::OrdinalIgnoreCase))) {
        return $null
    }
    $executable = $identity.executable
    if ([string]::IsNullOrEmpty($executable) -or [string]::IsNullOrEmpty($env:ProgramFiles)) {
        throw 'Cannot establish the hosted runner terminal executable identity'
    }
    if ($executable.StartsWith('\\?\', [StringComparison]::Ordinal)) { $executable = $executable.Substring(4) }
    $executable = [IO.Path]::GetFullPath($executable)
    $packageRoot = [IO.Path]::GetFullPath((Join-Path $env:ProgramFiles 'WindowsApps')).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    if ($identity.process_name -cne 'WindowsTerminal' -or
        -not $executable.StartsWith($packageRoot, [StringComparison]::OrdinalIgnoreCase) -or
        $executable.Substring($packageRoot.Length) -cnotmatch '^Microsoft\.WindowsTerminal_\d+\.\d+\.\d+\.\d+_arm64__8wekyb3d8bbwe[\\/]WindowsTerminal\.exe$') {
        throw 'Refusing a terminal outside its Microsoft ARM64 WindowsApps package'
    }
    $identity
}

function Hide-ObservedHostedTerminalWindows {
    # The ARM64 runner image can open a WSL update prompt after GUI readiness,
    # including a Terminal window with the application's default title:
    # https://github.com/actions/runner-images/issues/14264
    # Hide that exact observed window; leave the terminal and update running.
    $candidates = [Collections.Generic.List[object]]::new()
    $seen = [Collections.Generic.HashSet[long]]::new()
    foreach ($observed in @($Window, [HostedArmAccountWindow]::GetForegroundWindow().ToInt64())) {
        if ($observed -eq 0 -or -not $seen.Add($observed)) { continue }
        $candidate = Get-HostedTerminalWindow ([IntPtr]::new($observed))
        if ($null -ne $candidate) { $candidates.Add($candidate) }
    }
    $report.terminal_windows = @($report.terminal_windows) + @($candidates.ToArray())
    foreach ($candidate in $candidates) {
        $window = [IntPtr]::new($candidate.hwnd)
        $current = Get-HostedTerminalWindow $window
        if ($null -eq $current -or $current.process_id -ne $candidate.process_id -or
            $current.title -cne $candidate.title -or $current.process_name -cne $candidate.process_name -or
            -not [string]::Equals($current.executable, $candidate.executable, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Terminal identity changed before runner preparation; nothing was sent'
        }
        if (-not [HostedArmAccountWindow]::ShowWindowAsync($window, 0)) {
            throw 'The verified hosted runner terminal refused SW_HIDE'
        }
        $deadline = [DateTime]::UtcNow.AddSeconds(2)
        while ([HostedArmAccountWindow]::IsWindowVisible($window)) {
            [uint32]$owner = 0
            [void][HostedArmAccountWindow]::GetWindowThreadProcessId($window, [ref]$owner)
            if ($owner -ne $candidate.process_id) { throw 'Terminal owner changed while waiting for SW_HIDE' }
            if ([DateTime]::UtcNow -ge $deadline) { throw 'The hosted runner terminal remained visible after SW_HIDE' }
            Start-Sleep -Milliseconds 50
        }
        $candidate | Add-Member -NotePropertyName method -NotePropertyValue 'SW_HIDE'
        $candidate | Add-Member -NotePropertyName status -NotePropertyValue 'hidden'
    }
}

$report = [ordered]@{
    run_id = $env:GITHUB_RUN_ID
    runner_arch = $env:RUNNER_ARCH
    status = 'inspecting'
    started_utc = [DateTime]::UtcNow.ToString('o')
    method = 'WM_CLOSE to exactly matched account/Start/Search windows; verified Start/Search fallback and Microsoft ARM64 Terminal SW_HIDE; no input, account action or process termination'
    windows = @()
    shell_windows = @()
    terminal_windows = @()
    foreground = $null
}
try {
    if ($IdentifyOnly) {


        $identity = Get-CoveringWindowIdentity ([IntPtr]::new($Window))
        $report.occluder = $identity
        $report.status = 'not_present'
        if (Test-Path -LiteralPath $evidenceFile) {
            try {
                $loaded = Get-Content -LiteralPath $evidenceFile -Raw | ConvertFrom-Json
                $loaded | Add-Member -NotePropertyName occluder -NotePropertyValue $identity -Force
                $report = $loaded
            } catch {

            }
        }
    } else {
    Close-ObservedShellWindows
    Hide-ObservedHostedTerminalWindows
    $accountWindows = [Collections.Generic.List[object]]::new()
    $inspectionErrors = [Collections.Generic.List[string]]::new()



    $foreground = [HostedArmAccountWindow]::GetForegroundWindow()
    $report.foreground = $foreground.ToInt64()
    if ($Window -ne 0) {


        $report.occluder = Get-CoveringWindowIdentity ([IntPtr]::new($Window))



        $named = Get-AccountWindow ([IntPtr]::new($Window))
        if ($null -ne $named) {
            $named | Add-Member -NotePropertyName observed_via -NotePropertyValue 'occluder'
            $accountWindows.Add($named)
        }
        $enumerated = $true
    } else {
    $foregroundCandidate = Get-AccountWindow $foreground
    if ($null -ne $foregroundCandidate) {
        $foregroundCandidate | Add-Member -NotePropertyName observed_via -NotePropertyValue 'foreground'
        $accountWindows.Add($foregroundCandidate)
    }
    $enumerated = [HostedArmAccountWindow]::EnumWindows({ param($window, $unused)
        try {
            $candidate = Get-AccountWindow $window
            if ($null -ne $candidate -and ($null -eq $foregroundCandidate -or $window -ne $foreground)) {
                $candidate | Add-Member -NotePropertyName observed_via -NotePropertyValue 'enumeration'
                $accountWindows.Add($candidate)
            }
            return $true
        } catch {
            $inspectionErrors.Add($_.Exception.Message)
            return $false
        }
    }, [IntPtr]::Zero)
    }
    $report.windows = @($accountWindows.ToArray())
    if ($inspectionErrors.Count -gt 0) { throw "Cannot establish account-window identity: $inspectionErrors" }
    if (-not $enumerated) { throw "Cannot enumerate hosted runner windows (Win32 error $([Runtime.InteropServices.Marshal]::GetLastWin32Error()))" }
    if ($accountWindows.Count -gt 1) { throw 'Refusing ambiguous Microsoft-account windows on the hosted runner' }
    if ($accountWindows.Count -eq 0) {
        $report.status = if ($report.shell_windows.Count -gt 0 -or $report.terminal_windows.Count -gt 0) { 'closed' } else { 'not_present' }
    } else {
        $candidate = $accountWindows[0]
        $expectedExecutable = Join-Path $env:WINDIR 'System32\WWAHost.exe'
        if ($candidate.process_name -ne 'WWAHost' -or
            -not [string]::Equals($candidate.executable, $expectedExecutable, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Refusing an account window whose process is not the system WWAHost executable'
        }
        $window = [IntPtr]::new($candidate.hwnd)
        $current = Get-AccountWindow $window
        if ($null -eq $current -or $current.process_id -ne $candidate.process_id -or
            -not [string]::Equals($current.executable, $expectedExecutable, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Account-window identity changed before runner preparation; nothing was sent'
        }
        [UIntPtr]$messageResult = [UIntPtr]::Zero
        $sent = [HostedArmAccountWindow]::SendMessageTimeout($window, 0x0010, [UIntPtr]::Zero, [IntPtr]::Zero, 3, 1500, [ref]$messageResult)
        if ($sent -eq [IntPtr]::Zero) {
            $messageError = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
            throw "Account-window WM_CLOSE was not acknowledged (Win32 error $messageError)"
        }
        $deadline = [DateTime]::UtcNow.AddSeconds(5)
        while ([HostedArmAccountWindow]::IsWindowVisible($window)) {
            [uint32]$owner = 0
            [void][HostedArmAccountWindow]::GetWindowThreadProcessId($window, [ref]$owner)
            if ($owner -ne $candidate.process_id) { break }
            if ([DateTime]::UtcNow -ge $deadline) { throw 'The hosted runner account window remained visible after WM_CLOSE' }
            Start-Sleep -Milliseconds 50
        }
        $report.status = 'closed'
    }
    # Closing the account dialog may reveal a different foreground shell.
    # Qualify that observed window before the fixture requests CAD focus.
    Close-ObservedShellWindows
    Hide-ObservedHostedTerminalWindows
    }
} catch {
    $report.status = 'failed'
    $report.error = $_.Exception.Message
    throw
} finally {
    if (-not $IdentifyOnly) {
        try {
            $after = [HostedArmAccountWindow]::GetForegroundWindow()
            $report.foreground_after = if ($after -eq [IntPtr]::Zero) { $null } else { Get-CoveringWindowIdentity $after }
        } catch {
            $report.foreground_observation_error = $_.Exception.Message
        }
    }
    $report.finished_utc = [DateTime]::UtcNow.ToString('o')
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($evidenceFile))
    [IO.File]::WriteAllText($evidenceFile, ($report | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
}
