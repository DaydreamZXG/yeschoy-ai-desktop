$ErrorActionPreference = 'Stop'
$root = 'C:\yeschoy-qa\status-read-20260909'
$taskName = 'YeschoyQALaunch'
$task = Get-ScheduledTask -TaskName $taskName
if ($task.Principal.UserId -ne 'yeschoyqa') { throw 'Not the isolated QA task' }
Export-ScheduledTask -TaskName $taskName | Set-Content "$root\original-launch-task.xml" -Encoding UTF8
$action = New-ScheduledTaskAction -Execute 'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe' -Argument '-NoProfile -ExecutionPolicy Bypass -File C:\yeschoy-qa\status-read-20260909\launch-probe.ps1'
Set-ScheduledTask -TaskName $taskName -Action $action | Out-Null
Start-ScheduledTask -TaskName $taskName
Get-ScheduledTaskInfo -TaskName $taskName | Select-Object LastRunTime, LastTaskResult | ConvertTo-Json | Set-Content "$root\task-state.json" -Encoding UTF8
