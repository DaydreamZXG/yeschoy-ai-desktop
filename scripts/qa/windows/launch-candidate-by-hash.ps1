param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9a-fA-F]{64}$')]
    [string]$ExpectedSha256,

    [string]$OutputPath = 'C:\yeschoy-qa\launch-candidate-result.txt'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$programRoot = Join-Path $env:LOCALAPPDATA 'Programs'
$candidate = Get-ChildItem -LiteralPath $programRoot -Directory -ErrorAction SilentlyContinue |
    ForEach-Object {
        Get-ChildItem -LiteralPath $_.FullName -Filter '*.exe' -File -ErrorAction SilentlyContinue
    } |
    Where-Object { $_.Length -gt 5MB -and $_.Length -lt 100MB } |
    Where-Object {
        (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash -eq $ExpectedSha256
    } |
    Select-Object -First 1

if ($null -eq $candidate) {
    throw "No installed executable matched SHA-256 $ExpectedSha256."
}

$process = Start-Process -FilePath $candidate.FullName -PassThru
@(
    "processId=$($process.Id)"
    "executable=$($candidate.FullName)"
    "userName=$([System.Security.Principal.WindowsIdentity]::GetCurrent().Name)"
) | Set-Content -LiteralPath $OutputPath -Encoding utf8
