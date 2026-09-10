$ErrorActionPreference = 'Stop'
$root = 'C:\yeschoy-qa\status-read-20260909'
$env:RUST_BACKTRACE = 'full'
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9236'
[ordered]@{user=[System.Security.Principal.WindowsIdentity]::GetCurrent().Name; profile=$env:USERPROFILE; sessionId=(Get-Process -Id $PID).SessionId} | ConvertTo-Json | Set-Content "$root\launch-context.json" -Encoding UTF8
$exe = "$root\yeschoy-desktop.exe"
if ((Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -ne '58737a89f35ca080862d72c4a9918bf84da478e828a9c56b8aea4f463a336a8d') { throw 'Unexpected probe artifact' }
$process = Start-Process -FilePath $exe -WorkingDirectory $root -RedirectStandardError "$root\app-stderr.txt" -RedirectStandardOutput "$root\app-stdout.txt" -PassThru
[ordered]@{processId=$process.Id; startedAt=(Get-Date).ToUniversalTime().ToString('o')} | ConvertTo-Json | Set-Content "$root\launch-result.json" -Encoding UTF8
