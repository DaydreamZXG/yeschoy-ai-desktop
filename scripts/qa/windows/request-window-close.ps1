param(
    [Parameter(Mandatory = $true)]
    [int]$TargetProcessId
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class CloseWindowTools {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetClassName(IntPtr hWnd, StringBuilder text, int count);

    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam);
}
'@

$script:matched = $false
$callback = [CloseWindowTools+EnumWindowsProc]{
    param([IntPtr]$windowHandle, [IntPtr]$unused)

    $windowProcessId = [uint32]0
    [void][CloseWindowTools]::GetWindowThreadProcessId($windowHandle, [ref]$windowProcessId)
    if ($windowProcessId -ne $TargetProcessId) {
        return $true
    }

    $className = [Text.StringBuilder]::new(256)
    [void][CloseWindowTools]::GetClassName($windowHandle, $className, $className.Capacity)
    if ($className.ToString() -ne 'Tauri Window') {
        return $true
    }

    [void][CloseWindowTools]::PostMessage(
        $windowHandle,
        0x0010,
        [IntPtr]::Zero,
        [IntPtr]::Zero
    )
    $script:matched = $true
    return $false
}

[void][CloseWindowTools]::EnumWindows($callback, [IntPtr]::Zero)
if (-not $script:matched) {
    throw "No Tauri window was found for process $TargetProcessId."
}
