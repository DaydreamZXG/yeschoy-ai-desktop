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
# The installer writes here; the two roots above and the test-only "开饭" root below are earlier names it must still stop.
$currentRoot = Join-Path $env:LOCALAPPDATA 'YesChoy'
$testRoot = Join-Path $env:LOCALAPPDATA '开饭'
# The internal builds made before the rename to YesChoy were called "野菜".
$priorRoot = Join-Path $env:LOCALAPPDATA '野菜'
$decoyRoot = Join-Path $env:RUNNER_TEMP 'yeschoy-upgrade-decoy'
# What the Tauri-generated "开饭" test build left on a machine besides its
# executable. The installer must clear all of it once the new files are written.
$testUninstaller = Join-Path $testRoot 'uninstall.exe'
$testShortcuts = @(
    (Join-Path ([Environment]::GetFolderPath('DesktopDirectory')) '开饭.lnk'),
    (Join-Path ([Environment]::GetFolderPath('Programs')) '开饭.lnk')
)
$testRegistryKeys = @(
    'HKCU:\Software\kaifan\开饭',
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\开饭',
    'HKCU:\Software\Classes\kaifan'
)
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
    $targets.Add((Start-LegacyFixture -Path (Join-Path $currentRoot 'yeschoy-desktop.exe')))
    $targets.Add((Start-LegacyFixture -Path (Join-Path $testRoot 'kaifan-desktop.exe')))
    $targets.Add((Start-LegacyFixture -Path (Join-Path $priorRoot 'yeschoy-desktop.exe')))
    $decoys.Add((Start-LegacyFixture -Path (Join-Path $decoyRoot '野菜API.exe')))
    $decoys.Add((Start-LegacyFixture -Path (Join-Path $decoyRoot 'kaifan-desktop.exe')))

    Set-Content -LiteralPath $testUninstaller -Value 'fixture'
    foreach ($shortcut in $testShortcuts) {
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $shortcut) | Out-Null
        Set-Content -LiteralPath $shortcut -Value 'fixture'
    }
    foreach ($key in $testRegistryKeys) {
        New-Item -Path $key -Force | Out-Null
    }
    Set-Item -LiteralPath 'HKCU:\Software\kaifan\开饭' -Value $testRoot

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

    $installedBinary = Join-Path $currentRoot 'yeschoy-desktop.exe'
    if (-not (Test-Path -LiteralPath $installedBinary -PathType Leaf)) {
        throw "Installer returned success without writing $installedBinary."
    }

    foreach ($leftover in @($testRoot) + $testShortcuts + $testRegistryKeys) {
        if (Test-Path -LiteralPath $leftover) {
            throw "The 开饭 test build was not cleared: $leftover is still there."
        }
    }
    # The decoy has the same executable name outside an owned directory.
    if (-not (Test-Path -LiteralPath (Join-Path $decoyRoot 'kaifan-desktop.exe') -PathType Leaf)) {
        throw 'A same-named file outside an owned install directory was deleted.'
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
    Remove-Item -LiteralPath $currentRoot -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $testRoot -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $priorRoot -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $decoyRoot -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath 'HKCU:\Software\野菜API' -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath 'HKCU:\Software\yeschoy\野菜API' -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\野菜API' -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop' -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath 'HKCU:\Software\yeschoy\YesChoyStandalone' -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath 'HKCU:\Software\yeschoy\YesChoy' -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\YesChoy' -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath 'HKCU:\Software\Kaifan' -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath 'HKCU:\Software\kaifan\开饭' -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath 'HKCU:\Software\Classes\kaifan' -Recurse -Force -ErrorAction SilentlyContinue
    foreach ($shortcut in $testShortcuts) {
        Remove-Item -LiteralPath $shortcut -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\开饭' -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\com.kaifan.desktop' -Recurse -Force -ErrorAction SilentlyContinue
}
