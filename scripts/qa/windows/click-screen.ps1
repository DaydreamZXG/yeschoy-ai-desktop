param(
    [Parameter(Mandatory = $true)]
    [int]$X,

    [Parameter(Mandatory = $true)]
    [int]$Y,

    [ValidateRange(1, 2)]
    [int]$ClickCount = 1
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Add-Type @'
using System;
using System.Runtime.InteropServices;

public static class MouseTools {
    [DllImport("user32.dll")]
    public static extern bool SetCursorPos(int x, int y);

    [DllImport("user32.dll")]
    public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extraInfo);
}
'@

[void][MouseTools]::SetCursorPos($X, $Y)
for ($click = 0; $click -lt $ClickCount; $click += 1) {
    [MouseTools]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    [MouseTools]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    if ($click + 1 -lt $ClickCount) {
        Start-Sleep -Milliseconds 80
    }
}
