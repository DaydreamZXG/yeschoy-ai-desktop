param(
    [string]$OutputPath = 'C:\yeschoy-qa\visible-windows.json'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class VisibleWindowTools {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hWnd);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetClassName(IntPtr hWnd, StringBuilder text, int count);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
}
'@

$script:windowItems = @()
$callback = [VisibleWindowTools+EnumWindowsProc]{
    param([IntPtr]$windowHandle, [IntPtr]$unused)

    if (-not [VisibleWindowTools]::IsWindowVisible($windowHandle)) {
        return $true
    }

    $title = [Text.StringBuilder]::new(1024)
    $className = [Text.StringBuilder]::new(256)
    [void][VisibleWindowTools]::GetWindowText($windowHandle, $title, $title.Capacity)
    [void][VisibleWindowTools]::GetClassName($windowHandle, $className, $className.Capacity)
    $processId = [uint32]0
    [void][VisibleWindowTools]::GetWindowThreadProcessId($windowHandle, [ref]$processId)
    $process = Get-Process -Id $processId -ErrorAction SilentlyContinue
    $script:windowItems += [ordered]@{
        handle = $windowHandle.ToInt64()
        processId = [int]$processId
        processName = if ($null -eq $process) { $null } else { $process.ProcessName }
        className = $className.ToString()
        title = $title.ToString()
    }
    return $true
}

[void][VisibleWindowTools]::EnumWindows($callback, [IntPtr]::Zero)
$script:windowItems | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $OutputPath -Encoding utf8
