$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
function Stop-Install([string] $reason) {
    @{ present = $false; path = ''; reason = $reason } | ConvertTo-Json -Compress
    exit 0
}
function Find-WorkBuddy {
    foreach ($root in @($env:LOCALAPPDATA, $env:ProgramFiles, ${env:ProgramFiles(x86)})) {
        if ([string]::IsNullOrWhiteSpace($root)) { continue }
        foreach ($relative in @('WorkBuddy\WorkBuddy.exe', 'Programs\WorkBuddy\WorkBuddy.exe')) {
            $candidate = Join-Path $root $relative
            if (Test-Path -LiteralPath $candidate -ErrorAction Stop) { return $candidate }
        }
    }
    return $null
}
try {
    $exeTool = $env:YESCHOY_PACKAGE_NAME -ceq 'WorkBuddy'
    if ($env:YESCHOY_INSTALL_ACTION -eq 'presence') {
        if ($exeTool) {
            @{ present = [bool](Find-WorkBuddy); path = '' } | ConvertTo-Json -Compress
            exit 0
        }
        $packages = @(Get-AppxPackage -Name $env:YESCHOY_PACKAGE_NAME -ErrorAction Stop)
        $name = if ($env:YESCHOY_PACKAGE_NAME -ceq 'OpenAI.Codex') { 'Codex' } else { 'Claude' }
        $vendorPath = if ($name -eq 'Codex') { 'OpenAI\Codex\Codex.exe' } else { 'Anthropic\Claude\Claude.exe' }
        $exists = $packages.Count -gt 0
        foreach ($root in @($env:LOCALAPPDATA, $env:ProgramFiles, ${env:ProgramFiles(x86)})) {
            if ([string]::IsNullOrWhiteSpace($root)) { continue }
            foreach ($relative in @("$name\$name.exe", "Programs\$name\$name.exe", "Programs\$name Desktop\$name.exe", $vendorPath)) {
                if (Test-Path -LiteralPath (Join-Path $root $relative) -ErrorAction Stop) { $exists = $true }
            }
        }
        @{ present = $exists; path = '' } | ConvertTo-Json -Compress
        exit 0
    }
    $file = $env:YESCHOY_PACKAGE_FILE
    if ((Get-Item -LiteralPath $file).Attributes -band [IO.FileAttributes]::ReparsePoint) { Stop-Install 'unsafe_cache' }
    if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ine $env:YESCHOY_PACKAGE_SHA256) { Stop-Install 'download_changed' }
    if ($exeTool) {
        if (-not $file.ToLower().EndsWith('.exe')) { Stop-Install 'invalid_download' }
        $signature = Get-AuthenticodeSignature -LiteralPath $file -ErrorAction Stop
        if ($signature.Status -ne 'Valid') { Stop-Install 'signature_invalid' }
        if ($signature.SignerCertificate.Subject -notmatch 'Tencent') { Stop-Install 'identity_mismatch' }
        if ($env:YESCHOY_INSTALL_ACTION -eq 'open') {
            if (Find-WorkBuddy) { Stop-Install 'already_installed' }
            if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ine $env:YESCHOY_PACKAGE_SHA256) { Stop-Install 'download_changed' }
            Start-Process -FilePath $file -ErrorAction Stop
            @{ present = $false; path = '' } | ConvertTo-Json -Compress
            exit 0
        }
        if ($env:YESCHOY_INSTALL_ACTION -ne 'confirm') { exit 18 }
        $installed = Find-WorkBuddy
        if (-not $installed) { @{ present = $false; path = '' } | ConvertTo-Json -Compress; exit 0 }
        @{ present = $true; path = $installed } | ConvertTo-Json -Compress
        exit 0
    }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($file)
    try {
        $entries = @($zip.Entries | Where-Object { $_.FullName -ceq 'AppxManifest.xml' })
        if ($entries.Count -ne 1 -or $entries[0].Length -gt 1048576) { Stop-Install 'invalid_download' }
        if (@($zip.Entries | Where-Object { $_.FullName -ceq 'AppxSignature.p7x' }).Count -ne 1) { Stop-Install 'signature_invalid' }
        $settings = New-Object System.Xml.XmlReaderSettings
        $settings.DtdProcessing = [System.Xml.DtdProcessing]::Prohibit
        $settings.XmlResolver = $null
        $stream = $entries[0].Open()
        $reader = [Xml.XmlReader]::Create($stream, $settings)
        try {
            $document = New-Object System.Xml.XmlDocument
            $document.XmlResolver = $null
            $document.Load($reader)
        } finally { $reader.Dispose(); $stream.Dispose() }
        $identity = $document.DocumentElement.SelectSingleNode('*[local-name()="Identity"]')
        if ($null -eq $identity -or $identity.GetAttribute('Name') -cne $env:YESCHOY_PACKAGE_NAME -or
            $identity.GetAttribute('Publisher') -cne $env:YESCHOY_PACKAGE_PUBLISHER -or
            $identity.GetAttribute('ProcessorArchitecture') -cne $env:YESCHOY_PACKAGE_ARCH) { Stop-Install 'identity_mismatch' }
        $version = $identity.GetAttribute('Version')
        if ($version -notmatch '^\d+\.\d+\.\d+\.\d+$') { Stop-Install 'identity_mismatch' }
    } finally { $zip.Dispose() }
    # Manifest metadata is NOT proof of trust. Windows performs signature and
    # device-trust checks in its own installer; no policy or certificates change.
    if ($env:YESCHOY_INSTALL_ACTION -eq 'open') {
        if (@(Get-AppxPackage -Name $env:YESCHOY_PACKAGE_NAME -ErrorAction Stop).Count -gt 0) { Stop-Install 'already_installed' }
        if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ine $env:YESCHOY_PACKAGE_SHA256) { Stop-Install 'download_changed' }
        Start-Process -FilePath $file -ErrorAction Stop
        @{ present = $false; path = '' } | ConvertTo-Json -Compress
        exit 0
    }
    if ($env:YESCHOY_INSTALL_ACTION -ne 'confirm') { exit 18 }
    $packages = @(Get-AppxPackage -Name $env:YESCHOY_PACKAGE_NAME -ErrorAction Stop)
    if ($packages.Count -eq 0) { @{ present = $false; path = '' } | ConvertTo-Json -Compress; exit 0 }
    if ($packages.Count -ne 1) { Stop-Install 'installation_unconfirmed' }
    $package = $packages[0]
    if ($package.Publisher -cne $env:YESCHOY_PACKAGE_PUBLISHER -or
        $package.Version.ToString() -cne $version -or
        $package.Architecture.ToString() -ine $env:YESCHOY_PACKAGE_ARCH -or
        $package.Status.ToString() -ne 'Ok' -or
        $package.SignatureKind.ToString() -notin @('Store', 'Enterprise', 'Developer', 'System')) { Stop-Install 'installation_unconfirmed' }
    @{ present = $true; path = $package.InstallLocation } | ConvertTo-Json -Compress
} catch { exit 21 }
