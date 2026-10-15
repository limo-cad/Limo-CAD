param([int]$OwnedPid, [string]$Operation)
$ErrorActionPreference = 'Stop'
[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
if ($Operation -eq 'clipboard-read') {
    [Console]::Write([string](Get-Clipboard -Raw))
    exit 0
}
if ($Operation -eq 'clipboard-write') {
    $clipboardText = [Console]::In.ReadToEnd()
    if ($clipboardText.Length -eq 0) {
        # Windows PowerShell rejects empty Set-Clipboard text. A fresh runner
        # starts empty, so restoration must explicitly clear it (STA helper).
        Add-Type -AssemblyName System.Windows.Forms
        [System.Windows.Forms.Clipboard]::Clear()
    } else {
        Set-Clipboard -Value $clipboardText
    }
    exit 0
}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class NativePlatformInput {
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort key, scan; public uint flags, time; public UIntPtr extra; }
    [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int x, y; public uint data, flags, time; public UIntPtr extra; }
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x, y; }
    [StructLayout(LayoutKind.Explicit, Size = 32)] public struct UNION { [FieldOffset(0)] public KEYBDINPUT keyboard; [FieldOffset(0)] public MOUSEINPUT mouse; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public UNION data; }
    public delegate bool EnumProc(IntPtr window, IntPtr param);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback, IntPtr param);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr window, StringBuilder title, int count);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr window, StringBuilder name, int count);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window, int command);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr window, IntPtr after, int x, int y, int width, int height, uint flags);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out RECT rect);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window, out RECT rect);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window, ref POINT point);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT point);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT point);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint thread, uint attachedTo, bool attach);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll", SetLastError = true)] public static extern uint SendInput(uint count, INPUT[] input, int size);
    public static void Key(ushort key, bool up) {
        var input = new INPUT { type = 1, data = new UNION { keyboard = new KEYBDINPUT { key = key, flags = up ? 2u : 0u } } };
        if (SendInput(1, new[] { input }, Marshal.SizeOf(typeof(INPUT))) != 1) throw new InvalidOperationException("SendInput failed: " + Marshal.GetLastWin32Error());
        System.Threading.Thread.Sleep(25);
    }
    public static void Click() {
        var down = new INPUT { type = 0, data = new UNION { mouse = new MOUSEINPUT { flags = 2 } } };
        var up = new INPUT { type = 0, data = new UNION { mouse = new MOUSEINPUT { flags = 4 } } };
        if (SendInput(2, new[] { down, up }, Marshal.SizeOf(typeof(INPUT))) != 2) throw new InvalidOperationException("Mouse SendInput failed: " + Marshal.GetLastWin32Error());
    }
    public static void Mouse(uint flags, int data = 0) {
        var input = new INPUT { type = 0, data = new UNION { mouse = new MOUSEINPUT { flags = flags, data = unchecked((uint)data) } } };
        if (SendInput(1, new[] { input }, Marshal.SizeOf(typeof(INPUT))) != 1) throw new InvalidOperationException("Mouse SendInput failed: " + Marshal.GetLastWin32Error());
        System.Threading.Thread.Sleep(35);
    }
}
'@
[void][NativePlatformInput]::SetThreadDpiAwarenessContext([IntPtr]::new(-4))
$windows = [Collections.Generic.List[IntPtr]]::new()
[NativePlatformInput]::EnumWindows({ param($window, $unused)
    [uint32]$owner = 0
    [void][NativePlatformInput]::GetWindowThreadProcessId($window, [ref]$owner)
    if ($owner -eq $OwnedPid -and [NativePlatformInput]::IsWindowVisible($window)) {
        $class = [Text.StringBuilder]::new(512)
        [void][NativePlatformInput]::GetClassName($window, $class, 512)
        # Winit's event-loop message target is marked visible too; it is not
        # an application window and must never receive focus or keyboard input.
        if ($class.ToString() -ne 'Winit Thread Event Target') { $windows.Add($window) }
    }
    return $true
}, [IntPtr]::Zero) | Out-Null
if ($windows.Count -ne 1) {
    $descriptions = foreach ($window in $windows) {
        $title = [Text.StringBuilder]::new(512)
        $class = [Text.StringBuilder]::new(512)
        [void][NativePlatformInput]::GetWindowText($window, $title, 512)
        [void][NativePlatformInput]::GetClassName($window, $class, 512)
        "$window title=$title class=$class"
    }
    throw "Expected one visible window owned by PID $OwnedPid, got $($windows.Count): $descriptions"
}
[void][NativePlatformInput]::ShowWindow($windows[0], 9)
[uint32]$foregroundOwner = 0
$foregroundThread = [NativePlatformInput]::GetWindowThreadProcessId([NativePlatformInput]::GetForegroundWindow(), [ref]$foregroundOwner)
$helperThread = [NativePlatformInput]::GetCurrentThreadId()
$attached = $foregroundThread -ne $helperThread -and [NativePlatformInput]::AttachThreadInput($helperThread, $foregroundThread, $true)
try { [void][NativePlatformInput]::SetForegroundWindow($windows[0]) }
finally { if ($attached) { [void][NativePlatformInput]::AttachThreadInput($helperThread, $foregroundThread, $false) } }
if ([NativePlatformInput]::GetForegroundWindow() -ne $windows[0] -and $Operation -eq 'focus') {
    # Windows may deny background SetForegroundWindow even on an interactive
    # desktop. An actual click can activate our window. Never click a coordinate
    # until the OS confirms that the owned process is the recipient there.
    [void][NativePlatformInput]::SetWindowPos($windows[0], [IntPtr]::new(-1), 0, 0, 0, 0, 0x53)
    try {
        Start-Sleep -Milliseconds 100
        $rect = [NativePlatformInput+RECT]::new()
        if (-not [NativePlatformInput]::GetWindowRect($windows[0], [ref]$rect)) { throw 'Cannot locate owned native window' }
        $point = [NativePlatformInput+POINT]::new()
        $point.x = [int](($rect.left + $rect.right) / 2)
        $point.y = $rect.top + 16
        [uint32]$pointOwner = 0
        [void][NativePlatformInput]::GetWindowThreadProcessId([NativePlatformInput]::WindowFromPoint($point), [ref]$pointOwner)
        if ($pointOwner -ne $OwnedPid) { throw "Owned title bar is occluded at ($($point.x),$($point.y)) by PID $pointOwner; no mouse input was sent" }
        $previous = [NativePlatformInput+POINT]::new()
        [void][NativePlatformInput]::GetCursorPos([ref]$previous)
        if (-not [NativePlatformInput]::SetCursorPos($point.x, $point.y)) { throw 'Cannot move pointer to owned native title bar' }
        try { [NativePlatformInput]::Click(); Start-Sleep -Milliseconds 100 }
        finally { [void][NativePlatformInput]::SetCursorPos($previous.x, $previous.y) }
    } finally {
        [void][NativePlatformInput]::SetWindowPos($windows[0], [IntPtr]::new(-2), 0, 0, 0, 0, 0x53)
    }
}
$deadline = [DateTime]::UtcNow.AddSeconds(5)
while ([NativePlatformInput]::GetForegroundWindow() -ne $windows[0] -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
if ([NativePlatformInput]::GetForegroundWindow() -ne $windows[0]) {
    [uint32]$currentOwner = 0
    $current = [NativePlatformInput]::GetForegroundWindow()
    [void][NativePlatformInput]::GetWindowThreadProcessId($current, [ref]$currentOwner)
    throw "Cannot focus the owned native window; this runner needs an interactive desktop (target=$($windows[0]), foreground=$current, foreground PID=$currentOwner, input queues attached=$attached)"
}
if ($Operation -eq 'focus') { exit 0 }
if ($Operation -eq 'drawing-wheel' -or $Operation -eq 'drawing-pan') {
    $gesture = [Console]::In.ReadToEnd() | ConvertFrom-Json
    $clientRect = [NativePlatformInput+RECT]::new()
    if (-not [NativePlatformInput]::GetClientRect($windows[0], [ref]$clientRect)) { throw 'Cannot locate owned client rectangle' }
    $dpiScale = [NativePlatformInput]::GetDpiForWindow($windows[0]) / 96.0
    if ($dpiScale -le 0) { throw 'Owned window DPI unavailable' }
    function Move-OwnedPoint([double]$x, [double]$y) {
        if ([double]::IsNaN($x) -or [double]::IsInfinity($x) -or [double]::IsNaN($y) -or [double]::IsInfinity($y)) { throw 'Gesture coordinates must be finite' }
        $point = [NativePlatformInput+POINT]::new()
        $point.x = [int][Math]::Round($x * $dpiScale)
        $point.y = [int][Math]::Round($y * $dpiScale)
        if ($point.x -lt 0 -or $point.y -lt 0 -or $point.x -ge $clientRect.right -or $point.y -ge $clientRect.bottom) { throw 'Gesture point lies outside the owned client' }
        if (-not [NativePlatformInput]::ClientToScreen($windows[0], [ref]$point)) { throw 'Cannot map owned client coordinate' }
        [uint32]$pointOwner = 0
        [void][NativePlatformInput]::GetWindowThreadProcessId([NativePlatformInput]::WindowFromPoint($point), [ref]$pointOwner)
        if ($pointOwner -ne $OwnedPid -or [NativePlatformInput]::GetForegroundWindow() -ne $windows[0]) { throw 'Owned gesture target is occluded or lost focus; no input sent' }
        if (-not [NativePlatformInput]::SetCursorPos($point.x, $point.y)) { throw 'Cannot move owned pointer' }
        Start-Sleep -Milliseconds 35
    }
    Move-OwnedPoint $gesture.x $gesture.y
    if ($Operation -eq 'drawing-wheel') {
        $notches = [int]$gesture.notches
        if ($notches -eq 0 -or [Math]::Abs($notches) -gt 10) { throw 'Drawing wheel requires 1..10 signed notches' }
        if ($gesture.ctrl) { [NativePlatformInput]::Key(0x11, $false) }
        try { [NativePlatformInput]::Mouse(0x800, $notches * 120) }
        finally { if ($gesture.ctrl) { [NativePlatformInput]::Key(0x11, $true) } }
    } else {
        [NativePlatformInput]::Mouse(0x20)
        try {
            for ($step = 1; $step -le 6; $step++) {
                $amount = $step / 6.0
                Move-OwnedPoint ($gesture.x + ($gesture.to_x - $gesture.x) * $amount) ($gesture.y + ($gesture.to_y - $gesture.y) * $amount)
            }
        } finally { [NativePlatformInput]::Mouse(0x40) }
    }
    exit 0
}
$control = $Operation -ne 'right'
$key = switch ($Operation) { 'select-all' { 0x41 }; 'copy' { 0x43 }; 'paste' { 0x56 }; 'right' { 0x27 }; default { throw "Unknown input operation $Operation" } }
if ($control) { [NativePlatformInput]::Key(0x11, $false) }
try { [NativePlatformInput]::Key($key, $false); [NativePlatformInput]::Key($key, $true) }
finally { if ($control) { [NativePlatformInput]::Key(0x11, $true) } }
