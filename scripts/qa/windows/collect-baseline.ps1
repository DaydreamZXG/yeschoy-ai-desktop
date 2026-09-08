param(
    [string]$OutputPath = "$env:TEMP\yeschoy-qa-baseline.json"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$computer = Get-ComputerInfo
$system = Get-CimInstance Win32_ComputerSystem
$uac = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System'
$guestAgent = Get-Service qemu-ga -ErrorAction SilentlyContinue
$webViewRoot = "${env:ProgramFiles(x86)}\Microsoft\EdgeWebView\Application"

$result = [ordered]@{
    collectedAt = (Get-Date).ToUniversalTime().ToString('o')
    edition = $computer.WindowsProductName
    version = $computer.WindowsVersion
    build = [Environment]::OSVersion.Version.ToString()
    architecture = $env:PROCESSOR_ARCHITECTURE
    loggedOnUser = $system.UserName
    desktopShellRunning = [bool](Get-Process explorer -ErrorAction SilentlyContinue)
    uacEnabled = $uac.EnableLUA -eq 1
    webView2Present = Test-Path -LiteralPath $webViewRoot -PathType Container
    guestAgentStatus = if ($null -eq $guestAgent) { 'missing' } else { [string]$guestAgent.Status }
    freeDiskGiB = [math]::Round((Get-PSDrive C).Free / 1GB, 1)
}

$parent = Split-Path -Parent $OutputPath
if ($parent) {
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
}

$result | ConvertTo-Json | Set-Content -LiteralPath $OutputPath -Encoding utf8
