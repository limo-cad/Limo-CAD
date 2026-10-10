param(
    [Parameter(Mandatory = $true)][string]$EvidencePath,
    [Parameter(Mandatory = $true)][scriptblock]$Action
)
$ErrorActionPreference = 'Stop'

if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or
    $env:RUNNER_OS -ne 'Windows' -or $env:RUNNER_ARCH -ne 'ARM64' -or
    $env:GITHUB_REPOSITORY_ID -ne '1313334315' -or $env:GITHUB_RUN_ID -notmatch '^\d+$') {
    throw 'WSL isolation requires the disposable GitHub-hosted ARM64 package runner'
}
$runnerRoot = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
$evidenceFile = [IO.Path]::GetFullPath($EvidencePath)
if (-not $evidenceFile.StartsWith($runnerRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Runner evidence must stay inside RUNNER_TEMP'
}

# The runner agent repeatedly launches a broken WSL updater during GUI tests.
# Redirect only wsl.exe to Windows' inert systray stub for this test's lifetime.
# IFEO is shared across registry views on modern Windows. Use its canonical
# location once; deleting it twice through WOW6432Node aliases breaks cleanup.
# A foreground-window hide alone cannot prevent a new terminal appearing
# between observation and OS input.
# https://github.com/actions/runner-images/issues/14264
# https://learn.microsoft.com/en-us/windows/win32/winprog64/shared-registry-keys
$stub = Join-Path $env:WINDIR 'System32\systray.exe'
$redirect = '"' + $stub + '"'
$paths = @(
    'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\wsl.exe'
)
$states = [Collections.Generic.List[object]]::new()
$report = [ordered]@{
    run_id = $env:GITHUB_RUN_ID
    started_utc = [DateTime]::UtcNow.ToString('o')
    status = 'preparing'
    method = 'Scoped wsl.exe IFEO redirection at the shared canonical key; restored in finally'
    keys = @()
    existing_wsl = @()
}
try {
    if (-not (Test-Path -LiteralPath $stub -PathType Leaf)) {
        throw 'The system systray stub is missing; no registry settings were changed'
    }
    # Existing debugger policies belong to the runner and must not be overwritten.
    foreach ($path in $paths) {
        $exists = Test-Path -LiteralPath $path
        if ($exists -and ((Get-Item -LiteralPath $path).GetValueNames() -contains 'Debugger')) {
            throw "An existing wsl.exe debugger policy prevents isolation: $path"
        }
        $states.Add([pscustomobject]@{ path = $path; existed = $exists; created = $false; redirected = $false; restored = $false })
    }
    foreach ($state in $states) {
        if (-not $state.existed) {
            New-Item -Path $state.path -Force | Out-Null
            $state.created = $true
        }
        New-ItemProperty -LiteralPath $state.path -Name Debugger -PropertyType String -Value $redirect | Out-Null
        $state.redirected = $true
        if ((Get-ItemPropertyValue -LiteralPath $state.path -Name Debugger) -cne $redirect) {
            throw 'WSL isolation was not acknowledged; no input test was started'
        }
    }
    # A prompt launched before isolation can still create its terminal later.
    # Let those processes finish before clearing existing windows and starting
    # input. This bounded wait sends no keys and never kills a process.
    $updaters = @(Get-Process -Name wsl -ErrorAction SilentlyContinue)
    $report.existing_wsl = @($updaters | ForEach-Object { [pscustomobject]@{ process_id = $_.Id; finished = $false } })
    $deadline = [DateTime]::UtcNow.AddSeconds(90)
    for ($i = 0; $i -lt $updaters.Count; $i++) {
        $remaining = [int][Math]::Max(1, ($deadline - [DateTime]::UtcNow).TotalMilliseconds)
        if (-not $updaters[$i].WaitForExit($remaining)) {
            throw 'A preexisting WSL updater did not finish within 90s; no input test was started'
        }
        $report.existing_wsl[$i].finished = $true
    }
    $report.status = 'isolated'
    & $Action
    $report.status = 'passed'
} catch {
    $report.status = 'failed'
    $report.error = $_.Exception.Message
    throw
} finally {
    $restoreErrors = [Collections.Generic.List[string]]::new()
    foreach ($state in $states) {
        try {
            if ($state.redirected) {
                if ((Get-ItemPropertyValue -LiteralPath $state.path -Name Debugger) -cne $redirect) {
                    throw "WSL debugger policy changed during the test; refusing to remove another policy: $($state.path)"
                }
                Remove-ItemProperty -LiteralPath $state.path -Name Debugger
            }
            if ($state.created) {
                $key = Get-Item -LiteralPath $state.path
                if ($key.GetValueNames().Count -eq 0 -and $key.GetSubKeyNames().Count -eq 0) {
                    Remove-Item -LiteralPath $state.path
                }
            }
            $state.restored = $true
        } catch {
            $restoreErrors.Add($_.Exception.Message)
        }
    }
    $report.keys = @($states.ToArray())
    $report.restored = $restoreErrors.Count -eq 0
    if ($restoreErrors.Count -gt 0) {
        $report.status = 'failed'
        $report.restore_errors = @($restoreErrors.ToArray())
    }
    $report.finished_utc = [DateTime]::UtcNow.ToString('o')
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($evidenceFile))
    [IO.File]::WriteAllText($evidenceFile, ($report | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
    if ($restoreErrors.Count -gt 0) {
        $failure = "WSL isolation cleanup failed: $($restoreErrors -join '; ')"
        if ($report.error) { $failure += "; original failure: $($report.error)" }
        throw $failure
    }
}
