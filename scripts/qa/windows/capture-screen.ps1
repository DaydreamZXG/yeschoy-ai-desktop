param(
    [string]$OutputPath = 'C:\yeschoy-qa\screen.png'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class CaptureWindowTools {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetClassName(IntPtr hWnd, StringBuilder text, int count);

    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr hWnd, int command);
}
'@

$minimizeConsole = [CaptureWindowTools+EnumWindowsProc]{
    param([IntPtr]$windowHandle, [IntPtr]$unused)

    $className = [Text.StringBuilder]::new(256)
    [void][CaptureWindowTools]::GetClassName($windowHandle, $className, $className.Capacity)
    if ($className.ToString() -in @('CASCADIA_HOSTING_WINDOW_CLASS', 'ConsoleWindowClass')) {
        [void][CaptureWindowTools]::ShowWindow($windowHandle, 6)
    }
    return $true
}
[void][CaptureWindowTools]::EnumWindows($minimizeConsole, [IntPtr]::Zero)
Start-Sleep -Milliseconds 500

$bounds = [System.Windows.Forms.SystemInformation]::VirtualScreen
$bitmap = [System.Drawing.Bitmap]::new($bounds.Width, $bounds.Height)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
try {
    $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
    $bitmap.Save($OutputPath, [System.Drawing.Imaging.ImageFormat]::Png)
}
finally {
    $graphics.Dispose()
    $bitmap.Dispose()
}
