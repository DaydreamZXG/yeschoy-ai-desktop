param(
    [string]$OutputPath = 'C:\yeschoy-qa\openai-packages.json'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

@(
    Get-AppxPackage |
        Where-Object { $_.Name -match 'Codex|ChatGPT|OpenAI' } |
        Select-Object Name, PackageFullName, Version, InstallLocation
) | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $OutputPath -Encoding utf8
