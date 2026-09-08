param(
    [string]$OutputPath = "$env:TEMP\yeschoy-qa-app-state.json"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$knownExecutables = @('yeschoy-desktop.exe', 'CC Switch.exe', 'cc-switch.exe')
$userProgramRoot = 'C:\Users\yeschoyqa\AppData\Local\Programs\'
$processes = @(
    Get-CimInstance Win32_Process |
        Where-Object {
            ($knownExecutables -contains $_.Name) -or
            ($null -ne $_.ExecutablePath -and $_.ExecutablePath.StartsWith($userProgramRoot, [StringComparison]::OrdinalIgnoreCase))
        } |
        ForEach-Object {
            $desktopProcess = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
            [ordered]@{
                processId = [int]$_.ProcessId
                name = $_.Name
                executablePath = $_.ExecutablePath
                commandLine = $_.CommandLine
                sessionId = [int]$_.SessionId
                responding = if ($null -eq $desktopProcess) { $null } else { [bool]$desktopProcess.Responding }
                mainWindowTitle = if ($null -eq $desktopProcess) { $null } else { $desktopProcess.MainWindowTitle }
                mainWindowHandle = if ($null -eq $desktopProcess) { 0 } else { [int64]$desktopProcess.MainWindowHandle }
            }
        }
)

$gateway = Get-NetTCPConnection -LocalPort 15729 -State Listen -ErrorAction SilentlyContinue
$result = [ordered]@{
    collectedAt = (Get-Date).ToUniversalTime().ToString('o')
    processes = $processes
    gatewayListening = [bool]$gateway
    gatewayOwnerProcessIds = @($gateway | Select-Object -ExpandProperty OwningProcess -Unique)
}

$parent = Split-Path -Parent $OutputPath
if ($parent) {
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
}

$result | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $OutputPath -Encoding utf8
