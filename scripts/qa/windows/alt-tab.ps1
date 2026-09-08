Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Add-Type @'
using System;
using System.Runtime.InteropServices;

public static class KeyboardTools {
    [DllImport("user32.dll")]
    public static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extraInfo);
}
'@

[KeyboardTools]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
[KeyboardTools]::keybd_event(0x09, 0, 0, [UIntPtr]::Zero)
[KeyboardTools]::keybd_event(0x09, 0, 2, [UIntPtr]::Zero)
[KeyboardTools]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
