param([int]$PrintOwnedPid)
$ErrorActionPreference = 'Stop'
if ($env:LIMO_CAD_NATIVE_PRINT_TEST -ne 'windows-cancel' -or $env:GITHUB_ACTIONS -ne 'true' -or
    $env:RUNNER_OS -ne 'Windows' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or
    $env:GITHUB_REPOSITORY_ID -ne '1313334315' -or $env:GITHUB_RUN_ID -notmatch '^\d+$') {
    throw 'Native print Cancel requires an explicitly opted-in disposable GitHub Windows runner'
}
if ($PrintOwnedPid -le 0) { throw 'An owned native child PID is required' }
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class OwnedPrintCancel {
    public delegate bool EnumProc(IntPtr window, IntPtr param);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback, IntPtr param);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr window);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr window, uint command);
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr window, int id);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out RECT rect);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll", SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr window, uint message, UIntPtr wParam, IntPtr lParam, uint flags, uint timeout, out UIntPtr result);
    public static string Title(IntPtr window) { var s = new StringBuilder(512); GetWindowText(window, s, 512); return s.ToString(); }
    public static string Class(IntPtr window) { var s = new StringBuilder(512); GetClassName(window, s, 512); return s.ToString(); }
    public static uint Pid(IntPtr window) { uint pid; GetWindowThreadProcessId(window, out pid); return pid; }
}
'@
$printDeadline = [DateTime]::UtcNow.AddSeconds(12)
do {
    if (-not (Get-Process -Id $PrintOwnedPid -ErrorAction SilentlyContinue)) { throw 'Owned print host exited' }
    $printDialogs = [Collections.Generic.List[IntPtr]]::new()
    [void][OwnedPrintCancel]::EnumWindows({ param($window, $unused)
        if ([OwnedPrintCancel]::Pid($window) -eq $PrintOwnedPid -and
            [OwnedPrintCancel]::IsWindowVisible($window) -and [OwnedPrintCancel]::Class($window) -eq '#32770' -and
            [OwnedPrintCancel]::Title($window) -eq 'Print') { $printDialogs.Add($window) }
        return $true
    }, [IntPtr]::Zero)
    if ($printDialogs.Count -gt 1) { throw 'Multiple owned Print dialogs; no input was sent' }
    if ($printDialogs.Count -eq 1) { break }
    Start-Sleep -Milliseconds 100
} while ([DateTime]::UtcNow -lt $printDeadline)
if ($printDialogs.Count -ne 1) { throw 'No actual visible owned Print dialog appeared; no input was sent' }
$printDialog = $printDialogs[0]
$printOwner = [OwnedPrintCancel]::GetWindow($printDialog, 4)
$printCancel = [OwnedPrintCancel]::GetDlgItem($printDialog, 2)
if ($printOwner -eq [IntPtr]::Zero -or [OwnedPrintCancel]::Pid($printOwner) -ne $PrintOwnedPid -or
    [OwnedPrintCancel]::Class($printOwner) -eq '#32770' -or
    -not [OwnedPrintCancel]::IsWindowVisible($printOwner) -or
    [OwnedPrintCancel]::IsWindowEnabled($printOwner)) { throw 'Print dialog has no disabled native owner; no input was sent' }
if ($printCancel -eq [IntPtr]::Zero -or [OwnedPrintCancel]::Pid($printCancel) -ne $PrintOwnedPid -or
    [OwnedPrintCancel]::Class($printCancel) -ne 'Button' -or
    [OwnedPrintCancel]::Title($printCancel).Replace('&','') -ne 'Cancel' -or
    -not [OwnedPrintCancel]::IsWindowVisible($printCancel) -or -not [OwnedPrintCancel]::IsWindowEnabled($printCancel)) {
    throw 'Owned Print dialog has no enabled Cancel button; no input was sent'
}
$printRect = [OwnedPrintCancel+RECT]::new()
if (-not [OwnedPrintCancel]::GetWindowRect($printDialog, [ref]$printRect)) { throw 'Cannot observe print dialog bounds' }
$printEvidence = [ordered]@{ owned_pid=$PrintOwnedPid; dialog=$printDialog.ToInt64(); owner=$printOwner.ToInt64();
    title=[OwnedPrintCancel]::Title($printDialog); class=[OwnedPrintCancel]::Class($printDialog);
    bounds=@($printRect.left,$printRect.top,$printRect.right,$printRect.bottom);
    cancel_control=$printCancel.ToInt64(); cancel_control_id=2; action='cancel'; dialog_closed=$false;
    method='BM_CLICK on the verified owned IDCANCEL; no global input or Print action' }
[UIntPtr]$printResult = [UIntPtr]::Zero
if ([OwnedPrintCancel]::SendMessageTimeout($printCancel, 0xF5, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$printResult) -eq [IntPtr]::Zero) {
    throw "Owned Cancel click failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
}
$printDeadline = [DateTime]::UtcNow.AddSeconds(3)
while ([OwnedPrintCancel]::IsWindow($printDialog) -and [DateTime]::UtcNow -lt $printDeadline) { Start-Sleep -Milliseconds 50 }
if ([OwnedPrintCancel]::IsWindow($printDialog)) { throw 'Owned Print dialog remained open after Cancel' }
$printEvidence.dialog_closed = $true
$printEvidence | ConvertTo-Json -Depth 4 -Compress
