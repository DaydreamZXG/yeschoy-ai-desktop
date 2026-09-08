param(
    [Parameter(Mandatory = $true)]
    [string]$ProcessName
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class WindowTools {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);

    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr hWnd, int command);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);
}
'@

$targetProcessIds = @(
    Get-Process -Name $ProcessName -ErrorAction Stop |
        Select-Object -ExpandProperty Id
)

$shown = 0
$callback = [WindowTools+EnumWindowsProc]{
    param([IntPtr]$windowHandle, [IntPtr]$unused)

    $windowProcessId = [uint32]0
    [void][WindowTools]::GetWindowThreadProcessId($windowHandle, [ref]$windowProcessId)
    if ($targetProcessIds -contains [int]$windowProcessId) {
        [void][WindowTools]::ShowWindow($windowHandle, 9)
        [void][WindowTools]::SetForegroundWindow($windowHandle)
        $script:shown += 1
    }
    return $true
}

[void][WindowTools]::EnumWindows($callback, [IntPtr]::Zero)
Write-Output "shownWindows=$shown"
