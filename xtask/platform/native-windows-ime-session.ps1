param([int]$ImeOwnedPid, [long]$ImeWindow)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
function Resolve-OwnedImePaths([string]$RunnerRoot, [string]$OutputRoot, [string]$HostPath, [string]$OwnedPath) {
    $canonicalRoot = [NativePlatformInput]::CanonicalPath($RunnerRoot).TrimEnd('\') + '\'
    $canonicalOutput = [NativePlatformInput]::CanonicalPath($OutputRoot).TrimEnd('\')
    if (-not $canonicalOutput.StartsWith($canonicalRoot, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'IME output must be beneath RUNNER_TEMP'
    }
    $expectedHost = [NativePlatformInput]::CanonicalPath($HostPath)
    $actualHost = [NativePlatformInput]::CanonicalPath($OwnedPath)
    if (-not [string]::Equals($actualHost, $expectedHost, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Owned PID executable does not match the launched native host'
    }
    return $canonicalOutput
}


if ($env:LIMO_CAD_NATIVE_IME_TEST -ne 'windows-japanese' -or $env:GITHUB_ACTIONS -ne 'true' -or
    $env:RUNNER_OS -ne 'Windows' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or
    $env:GITHUB_REPOSITORY_ID -ne '1313334315' -or $env:GITHUB_RUN_ID -notmatch '^\d+$') {
    throw 'Explicit disposable GitHub Windows IME opt-in is required'
}
if (-not $env:LIMO_CAD_IME_SESSION -or -not $env:LIMO_CAD_IME_FIELD_TOKEN) { throw 'Missing document/field receipt' }
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class NativePlatformInput {
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern Microsoft.Win32.SafeHandles.SafeFileHandle CreateFileW(string path, uint access, uint share, IntPtr security, uint creation, uint flags, IntPtr template);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern uint GetFinalPathNameByHandleW(Microsoft.Win32.SafeHandles.SafeFileHandle file, StringBuilder path, uint size, uint flags);
    public static string CanonicalPath(string path) {
        using (var file = CreateFileW(System.IO.Path.GetFullPath(path), 0, 7, IntPtr.Zero, 3, 0x02000000, IntPtr.Zero)) {
            if (file.IsInvalid) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "Cannot open owned path: " + path);
            var result = new StringBuilder(1024);
            uint length = GetFinalPathNameByHandleW(file, result, (uint)result.Capacity, 0);
            if (length == 0) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "Cannot resolve owned path: " + path);
            if (length >= result.Capacity) {
                if (length > 32768) throw new System.IO.PathTooLongException(path);
                result = new StringBuilder((int)length + 1);
                length = GetFinalPathNameByHandleW(file, result, (uint)result.Capacity, 0);
                if (length == 0) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "Cannot resolve owned path: " + path);
                if (length >= result.Capacity) throw new System.IO.IOException("Owned path changed during resolution: " + path);
            }
            return result.ToString();
        }
    }
    [StructLayout(LayoutKind.Sequential)] private struct KEYBDINPUT { public ushort key, scan; public uint flags, time; public UIntPtr extra; }
    [StructLayout(LayoutKind.Explicit, Size = 32)] private struct UNION { [FieldOffset(0)] public KEYBDINPUT keyboard; }
    [StructLayout(LayoutKind.Sequential)] private struct INPUT { public uint type; public UNION data; }
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern IntPtr GetKeyboardLayout(uint thread);
    [DllImport("user32.dll", SetLastError = true)] public static extern IntPtr SendMessageTimeout(IntPtr window, uint message, UIntPtr wParam, IntPtr lParam, uint flags, uint timeout, out UIntPtr result);
    [DllImport("user32.dll", SetLastError = true)] private static extern uint SendInput(uint count, INPUT[] input, int size);
    public static void Key(ushort key, bool up) {
        var input = new INPUT { type = 1, data = new UNION { keyboard = new KEYBDINPUT { key = key, flags = up ? 2u : 0u } } };
        if (SendInput(1, new[] { input }, Marshal.SizeOf(typeof(INPUT))) != 1) throw new InvalidOperationException("SendInput failed: " + Marshal.GetLastWin32Error());
        System.Threading.Thread.Sleep(25);
    }
}
'@
$ImeWindowHandle = [IntPtr]::new($ImeWindow)
$owned = Get-Process -Id $ImeOwnedPid -ErrorAction Stop
$ownedStart = $owned.StartTime
$outputRoot = Resolve-OwnedImePaths $env:RUNNER_TEMP $env:LIMO_CAD_IME_OUT $env:LIMO_CAD_IME_HOST_PATH $owned.MainModule.FileName
[uint32]$windowOwner = 0
$windowThread = [NativePlatformInput]::GetWindowThreadProcessId($ImeWindowHandle, [ref]$windowOwner)
if ($windowOwner -ne $ImeOwnedPid -or $windowThread -eq 0) { throw 'Owned window thread is absent' }
$previousLayout = [NativePlatformInput]::GetKeyboardLayout($windowThread)
if ($previousLayout -eq [IntPtr]::Zero) { throw 'Cannot record previous host input layout' }
$source = 'a76c93d9-5523-4e90-aafa-4db112f9ac76'
$errors = [Collections.Generic.List[string]]::new()
$operations = [Collections.Generic.List[object]]::new()
$cleanup = @{ layout_restored = $false; errors = $errors }
$result = 'failed'
$sequence = 0L
$finishRequested = $false
function Require-Owned([bool]$Foreground = $true) {
    $current = Get-Process -Id $ImeOwnedPid -ErrorAction Stop
    [uint32]$ownerNow = 0
    $threadNow = [NativePlatformInput]::GetWindowThreadProcessId($ImeWindowHandle, [ref]$ownerNow)
    if ($current.StartTime -ne $ownedStart -or $ownerNow -ne $ImeOwnedPid -or $threadNow -ne $windowThread -or
        -not [NativePlatformInput]::IsWindowVisible($ImeWindowHandle)) { throw 'Owned host/window identity changed' }
    if ($Foreground -and [NativePlatformInput]::GetForegroundWindow() -ne $ImeWindowHandle) { throw 'Owned native window lost focus; no further key input sent' }
}
function Language { return ([NativePlatformInput]::GetKeyboardLayout($windowThread).ToInt64() -band 0xffff) }
function Reply($Value) { [Console]::WriteLine(($Value | ConvertTo-Json -Depth 12 -Compress)) }
function Chord([uint16]$Modifier, [uint16]$Key) {
    Require-Owned
    if ($Modifier -ne 0) { [NativePlatformInput]::Key($Modifier, $false) }
    try {
        Require-Owned
        [NativePlatformInput]::Key($Key, $false)
        [NativePlatformInput]::Key($Key, $true)
    } finally { if ($Modifier -ne 0) { [NativePlatformInput]::Key($Modifier, $true) } }
}
try {
    Require-Owned
    Reply @{ status = 'ready'; source_id = $source; window_number = $ImeWindowHandle.ToInt64(); owned_pid = $ImeOwnedPid; previous_layout = $previousLayout.ToInt64() }
    while ($null -ne ($line = [Console]::ReadLine())) {
        if ($line.Length -gt 65536) { throw 'IME request exceeds byte budget' }
        $request = $line | ConvertFrom-Json
        $sequence++
        if ($request.sequence -ne $sequence) { throw 'Stale or out-of-order IME request' }
        if ($request.operation -eq 'finish') { $finishRequested = $true; break }
        $age = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - [double]$request.checked_unix_ms
        if ($age -lt -100 -or $age -gt 5000 -or $request.session -ne $env:LIMO_CAD_IME_SESSION -or
            $request.field_token -ne $env:LIMO_CAD_IME_FIELD_TOKEN -or -not $request.focused_control) { throw 'Stale or mismatched owned field receipt' }
        Require-Owned
        switch ($request.operation) {
            'enable' {
                for ($attempt = 0; (Language) -ne 0x411 -and $attempt -lt 8; $attempt++) {
                    Chord 0x5b 0x20
                    Start-Sleep -Milliseconds 400
                }
                if ((Language) -ne 0x411) { throw 'Japanese did not become the owned host thread input language' }
                Chord 0x11 0x14
            }
            'preedit' {
                if ((Language) -ne 0x411) { throw 'Owned host input language changed before preedit' }
                foreach ($key in @(0x48, 0x41, 0x52, 0x55)) { Chord 0 $key }
            }
            'commit' { Chord 0 0x0d }
            'escape' { Chord 0 0x1b }
            default { throw "Unknown IME operation $($request.operation)" }
        }
        Require-Owned
        $reply = @{ status = 'applied'; sequence = $sequence; operation = $request.operation; language = (Language); window_number = $ImeWindowHandle.ToInt64() }
        $operations.Add($reply)
        Reply $reply
    }
    if ($finishRequested) { $result = 'passed' }
} catch {
    $errors.Add($_.Exception.Message)
} finally {
    try {
        Require-Owned $false
        if ([NativePlatformInput]::GetKeyboardLayout($windowThread) -ne $previousLayout) {
            [UIntPtr]$messageResult = [UIntPtr]::Zero
            $sent = [NativePlatformInput]::SendMessageTimeout($ImeWindowHandle, 0x50, [UIntPtr]::Zero, $previousLayout, 2, 1000, [ref]$messageResult)
            if ($sent -eq [IntPtr]::Zero) { throw 'Owned host layout restoration message failed' }
        }
        $deadline = [DateTime]::UtcNow.AddSeconds(2)
        while ([NativePlatformInput]::GetKeyboardLayout($windowThread) -ne $previousLayout -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 25 }
        $cleanup.layout_restored = [NativePlatformInput]::GetKeyboardLayout($windowThread) -eq $previousLayout
        if (-not $cleanup.layout_restored) { throw 'Owned host layout restoration was not observed' }
    } catch { $errors.Add($_.Exception.Message) }
    if ($errors.Count) { $result = 'failed' }
    $report = @{ status = 'finished'; result = $result; cleanup = $cleanup; operations = $operations; owned_pid = $ImeOwnedPid; window_number = $ImeWindowHandle.ToInt64() }
    [IO.File]::WriteAllText([IO.Path]::Combine($outputRoot, 'windows-ime-cleanup.json'), ($report | ConvertTo-Json -Depth 12), [Text.UTF8Encoding]::new($false))
    Reply $report
}
if ($result -ne 'passed') { exit 1 }
exit 0
