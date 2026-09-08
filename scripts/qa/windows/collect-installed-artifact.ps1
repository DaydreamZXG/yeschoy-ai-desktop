param(
    [string]$OutputPath = 'C:\yeschoy-qa\installed-artifact.json'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$programRoot = Join-Path $env:LOCALAPPDATA 'Programs'
$applications = @(
    Get-ChildItem -LiteralPath $programRoot -Directory -ErrorAction SilentlyContinue |
        ForEach-Object {
            Get-ChildItem -LiteralPath $_.FullName -Filter '*.exe' -File -ErrorAction SilentlyContinue
        } |
        Where-Object {
            $_.Length -gt 5MB -and $_.Length -lt 100MB -and $_.Name -notmatch 'uninstall'
        }
)
$result = [ordered]@{
    collectedAt = (Get-Date).ToUniversalTime().ToString('o')
    userName = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
    programRoot = $programRoot
    candidates = @(
        $applications | ForEach-Object {
            [ordered]@{
                executablePath = $_.FullName
                sizeBytes = [int64]$_.Length
                sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
                productVersion = $_.VersionInfo.ProductVersion
                fileDescription = $_.VersionInfo.FileDescription
            }
        }
    )
}

$result | ConvertTo-Json | Set-Content -LiteralPath $OutputPath -Encoding utf8
