param(
    [string]$OutputPath = "$env:TEMP\yeschoy-qa-processes.json"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$processes = @(
    Get-CimInstance Win32_Process |
        ForEach-Object {
            [ordered]@{
                processId = [int]$_.ProcessId
                parentProcessId = [int]$_.ParentProcessId
                sessionId = [int]$_.SessionId
                name = $_.Name
                executablePath = $_.ExecutablePath
                commandLine = $_.CommandLine
            }
        }
)

$parent = Split-Path -Parent $OutputPath
if ($parent) {
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
}

$processes | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $OutputPath -Encoding utf8
