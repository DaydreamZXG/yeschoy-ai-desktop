param(
    [Parameter(Mandatory = $true)]
    [string]$InstallerPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_TEMP -eq '') {
    throw 'This destructive installation smoke test may run only on an ephemeral GitHub Actions runner.'
}

$installer = (Resolve-Path -LiteralPath $InstallerPath).Path
$standaloneRoot = Join-Path $env:LOCALAPPDATA 'Programs\野菜API'
$tauriRoot = Join-Path $env:LOCALAPPDATA '野菜API'
$decoyRoot = Join-Path $env:RUNNER_TEMP 'yeschoy-upgrade-decoy'
$targets = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()
$decoys = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()

function Start-LegacyFixture {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Path) | Out-Null
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        Copy-Item -LiteralPath "$env:SystemRoot\System32\ping.exe" -Destination $Path
    }
    return Start-Process `
        -FilePath $Path `
        -ArgumentList '-t', '127.0.0.1' `
        -WindowStyle Hidden `
        -PassThru
}

function Assert-Exited {
    param(
        [Parameter(Mandatory = $true)]
        [System.Diagnostics.Process]$Process,
        [Parameter(Mandatory = $true)]
        [string]$Label
    )

    try {
        $Process.WaitForExit(5000) | Out-Null
        $Process.Refresh()
    }
    catch {
        # Refresh can race with final handle teardown; Get-Process below is the
        # authoritative check for the original PID.
    }
    if ($null -ne (Get-Process -Id $Process.Id -ErrorAction SilentlyContinue)) {
        throw "$Label process $($Process.Id) survived the installer upgrade guard."
    }
}

try {
    $targets.Add((Start-LegacyFixture -Path (Join-Path $standaloneRoot '野菜API.exe')))
    $targets.Add((Start-LegacyFixture -Path (Join-Path $tauriRoot 'yeschoy-desktop.exe')))
    $targets.Add((Start-LegacyFixture -Path (Join-Path $tauriRoot 'yeschoy-desktop.exe')))
    $decoys.Add((Start-LegacyFixture -Path (Join-Path $decoyRoot '野菜API.exe')))

    Start-Sleep -Seconds 1
    foreach ($process in @($targets) + @($decoys)) {
        if ($process.HasExited) {
            throw "Fixture process $($process.Id) exited before the upgrade test began."
        }
    }

    $installation = Start-Process -FilePath $installer -ArgumentList '/S' -Wait -PassThru
    if ($installation.ExitCode -ne 0) {
        throw "Silent upgrade exited with code $($installation.ExitCode)."
    }

    foreach ($process in $targets) {
        Assert-Exited -Process $process -Label 'Owned legacy'
    }
    foreach ($process in $decoys) {
        $process.Refresh()
        if ($process.HasExited) {
            throw "A same-named process outside an owned install directory was terminated."
        }
    }

    $installedBinary = Join-Path $tauriRoot 'yeschoy-desktop.exe'
    if (-not (Test-Path -LiteralPath $installedBinary -PathType Leaf)) {
        throw "Installer returned success without writing $installedBinary."
    }
}
finally {
    foreach ($process in @($targets) + @($decoys)) {
        if ($null -ne (Get-Process -Id $process.Id -ErrorAction SilentlyContinue)) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
        $process.Dispose()
    }

    Remove-Item -LiteralPath $standaloneRoot -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $tauriRoot -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $decoyRoot -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath 'HKCU:\Software\野菜API' -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath 'HKCU:\Software\yeschoy\野菜API' -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\野菜API' -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop' -Recurse -Force -ErrorAction SilentlyContinue
}
