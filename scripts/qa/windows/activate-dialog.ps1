param(
    [Parameter(Mandatory = $true)]
    [int]$TargetProcessId,

    [ValidateRange(0, 65535)]
    [int]$CommandId = 0
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class DialogTools {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetClassName(IntPtr hWnd, StringBuilder text, int count);

    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr hWnd, int command);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam);
}
'@

$script:matched = $false
$callback = [DialogTools+EnumWindowsProc]{
    param([IntPtr]$windowHandle, [IntPtr]$unused)

    $windowProcessId = [uint32]0
    [void][DialogTools]::GetWindowThreadProcessId($windowHandle, [ref]$windowProcessId)
    if ($windowProcessId -ne $TargetProcessId) {
        return $true
    }

    $className = [Text.StringBuilder]::new(256)
    [void][DialogTools]::GetClassName($windowHandle, $className, $className.Capacity)
    if ($className.ToString() -ne '#32770') {
        return $true
    }

    [void][DialogTools]::ShowWindow($windowHandle, 9)
    [void][DialogTools]::SetForegroundWindow($windowHandle)
    if ($CommandId -ne 0) {
        [void][DialogTools]::PostMessage($windowHandle, 0x0111, [IntPtr]$CommandId, [IntPtr]::Zero)
    }
    $script:matched = $true
    return $false
}

[void][DialogTools]::EnumWindows($callback, [IntPtr]::Zero)
if (-not $script:matched) {
    throw "No dialog window was found for process $TargetProcessId."
}
