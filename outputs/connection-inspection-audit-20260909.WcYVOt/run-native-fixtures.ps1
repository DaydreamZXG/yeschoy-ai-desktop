param(
    [ValidateSet('before', 'after')][string]$Stage,
    [ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ExpectedHash
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = 'C:\yeschoy-qa\status-read-20260909'
$exe = "$root\native-$Stage.exe"
Invoke-WebRequest -UseBasicParsing -TimeoutSec 60 -Uri "http://192.168.64.1:18437/$Stage.exe" -OutFile $exe
$actualHash = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actualHash -ne $ExpectedHash.ToLowerInvariant()) { throw 'Unexpected native test artifact' }
$env:RUST_BACKTRACE = '1'
# This selector runs only synthetic files and worker/projection regressions.
# It never starts Codex/Claude, reads real credentials, or modifies a real profile.
$process = Start-Process -FilePath $exe -ArgumentList 'inspection_regression', '--nocapture', '--test-threads=1' -WorkingDirectory $root -RedirectStandardOutput "$root\native-$Stage-stdout.txt" -RedirectStandardError "$root\native-$Stage-stderr.txt" -Wait -PassThru
[ordered]@{
    stage = $Stage
    sha256 = $actualHash
    exitCode = $process.ExitCode
    architecture = $env:PROCESSOR_ARCHITECTURE
    windowsVersion = [System.Environment]::OSVersion.Version.ToString()
    user = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
} | ConvertTo-Json | Set-Content "$root\native-$Stage-result.json" -Encoding UTF8
