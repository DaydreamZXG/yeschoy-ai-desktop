$ErrorActionPreference = 'Stop'
$machine = Get-CimInstance Win32_ComputerSystem
New-Item -ItemType Directory -Path 'C:\yeschoy-qa\status-read-20260909' -Force | Out-Null
[ordered]@{
    loggedOnUser = $machine.UserName
    architecture = $env:PROCESSOR_ARCHITECTURE
    qaProfilePresent = (Test-Path 'C:\Users\yeschoyqa')
    tasks = @(Get-ScheduledTask | Where-Object {$_.TaskName -like '*yeschoy*'} | ForEach-Object {
        [ordered]@{name=$_.TaskName; user=$_.Principal.UserId; logonType=[string]$_.Principal.LogonType}
    })
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath 'C:\yeschoy-qa\status-read-20260909\baseline.json' -Encoding UTF8
