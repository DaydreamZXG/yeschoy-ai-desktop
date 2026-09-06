use super::{cache, catalog::Source, Result, Worker};
#[cfg(target_os = "macos")]
use crate::tool_adapters;
use crate::{desktop_app_discovery, tool_adapters::common::run_bounded};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::process::Command;

pub(super) enum Installed {
    #[cfg(target_os = "macos")]
    Created(PathBuf),
    #[cfg(target_os = "windows")]
    SystemConfirmation,
}

async fn run(
    command: Command,
    reason: &'static str,
) -> Result<crate::tool_adapters::common::ProcessResult> {
    let output = run_bounded(command, Duration::from_secs(180))
        .await
        .map_err(|_| reason)?;
    if output.success {
        Ok(output)
    } else {
        Err(reason)
    }
}

pub(super) async fn present(source: Source) -> Result<bool> {
    let identity_found = !desktop_app_discovery::activation_candidates(source.tool).is_empty();
    #[cfg(target_os = "macos")]
    {
        let home = tool_adapters::user_home().ok_or("home_unavailable")?;
        mac_destination_presence(source, &home.join("Applications"), identity_found)
    }
    #[cfg(target_os = "windows")]
    {
        if identity_found {
            return Ok(true);
        }
        let output = windows(source, "presence", None, "").await?;
        Ok(output.present)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = identity_found;
        Err("unsupported_platform")
    }
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn mac_destination_presence(
    source: Source,
    applications: &Path,
    identity_found: bool,
) -> Result<bool> {
    // Canonical discovery checks CFBundleIdentifier, including a genuine Codex
    // named ChatGPT.app. A filename alone is neither identity nor installation.
    if identity_found {
        return Ok(true);
    }
    let name = if source.tool == "codex_desktop" {
        "Codex.app"
    } else {
        "Claude.app"
    };
    match std::fs::symlink_metadata(applications.join(name)) {
        Ok(_) => Err("installation_location_conflict"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("presence_unconfirmed"),
    }
}

pub(super) async fn install(
    worker: &Worker,
    source: Source,
    folder: &Path,
    file: &Path,
    hash: &str,
) -> Result<Installed> {
    if cache::digest(file)? != hash {
        return Err("download_changed");
    }
    if present(source).await? {
        return Err("already_installed");
    }
    worker.check_cancelled()?;
    #[cfg(target_os = "macos")]
    {
        mac_install(worker, source, folder, file).await
    }
    #[cfg(target_os = "windows")]
    {
        let snapshot = if file
            .parent()
            .is_some_and(|p| p.file_name().is_some_and(|n| n == "opened-packages"))
        {
            file.to_owned()
        } else {
            cache::handoff_package(folder, file, hash)?
        };
        worker.update(|job| job.package = Some((snapshot.clone(), hash.to_owned())));
        // This opens the local package in Windows App Installer. It does not
        // call deployment commands or claim that a signature has been trusted.
        windows(source, "open", Some(&snapshot), hash).await?;
        worker.phase("awaiting_system_confirmation", false);
        Ok(Installed::SystemConfirmation)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (worker, source, folder);
        Err("unsupported_platform")
    }
}

pub(super) async fn confirmed(source: Source, file: &Path, hash: &str) -> Result<PathBuf> {
    if cache::digest(file)? != hash {
        return Err("download_changed");
    }
    #[cfg(target_os = "windows")]
    {
        let result = windows(source, "confirm", Some(file), hash).await?;
        if !result.present || result.path.is_empty() {
            return Err("installation_not_detected");
        }
        let candidates: Vec<_> = desktop_app_discovery::activation_candidates(source.tool)
            .into_iter()
            .filter_map(|candidate| {
                desktop_app_discovery::resolve_launch_target(source.tool, &candidate.path)
                    .map(|launch| (candidate.path, launch))
            })
            .collect();
        confirmed_executable(source.tool, &PathBuf::from(result.path), &candidates)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = source;
        Err("invalid_installation_action")
    }
}

#[cfg(any(target_os = "windows", test))]
pub(super) fn confirmed_executable(
    tool: &str,
    root: &Path,
    candidates: &[(
        PathBuf,
        crate::desktop_app_discovery_core::DesktopLaunchTarget,
    )],
) -> Result<PathBuf> {
    use crate::desktop_app_discovery_core::{windows_manifest_applications, DesktopLaunchTarget};
    let root = std::fs::canonicalize(root).map_err(|_| "installation_unconfirmed")?;
    let manifest = root.join("AppxManifest.xml");
    if std::fs::metadata(&manifest)
        .map_err(|_| "installation_unconfirmed")?
        .len()
        > 1024 * 1024
    {
        return Err("installation_unconfirmed");
    }
    let bytes = std::fs::read(&manifest).map_err(|_| "installation_unconfirmed")?;
    let applications =
        windows_manifest_applications(tool, &bytes).ok_or("installation_unconfirmed")?;
    let mut matches = Vec::new();
    for (candidate, launch) in candidates {
        let DesktopLaunchTarget::WindowsPackage(identity) = launch else {
            continue;
        };
        let Some((_, relative_id)) = identity.app_user_model_id().split_once('!') else {
            continue;
        };
        for (id, executable) in &applications {
            if relative_id != id {
                continue;
            }
            let declared = executable
                .split(['/', '\\'])
                .fold(root.clone(), |path, part| path.join(part));
            let Ok(declared) = std::fs::canonicalize(declared) else {
                continue;
            };
            if declared.starts_with(&root)
                && declared.is_file()
                && std::fs::canonicalize(candidate).ok().as_ref() == Some(&declared)
            {
                matches.push(declared);
            }
        }
    }
    if matches.len() != 1 {
        return Err("installation_unconfirmed");
    }
    Ok(matches.remove(0))
}

pub(super) async fn open_guide(url: &'static str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("/usr/bin/open");
    #[cfg(target_os = "windows")]
    let command = {
        let mut c = powershell()?;
        c.env("YESCHOY_INSTALL_GUIDE", url).args([
            "-Command",
            "Start-Process -FilePath $env:YESCHOY_INSTALL_GUIDE -ErrorAction Stop",
        ]);
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = Command::new("xdg-open");
    #[cfg(not(target_os = "windows"))]
    command.arg(url);
    run(command, "guide_unavailable").await.map(|_| ())
}

#[cfg(target_os = "macos")]
async fn verify_app(source: Source, app: &Path) -> Result<()> {
    let mut verify = Command::new("/usr/bin/codesign");
    verify.args(["--verify", "--deep", "--strict"]).arg(app);
    run(verify, "signature_invalid").await?;
    let mut identity = Command::new("/usr/bin/codesign");
    identity.args(["-d", "--verbose=4"]).arg(app);
    let result = run(identity, "signature_invalid").await?;
    let info = String::from_utf8_lossy(&result.stderr);
    if !info
        .lines()
        .any(|line| line == format!("TeamIdentifier={}", source.publisher))
        || !info
            .lines()
            .any(|line| line == format!("Identifier={}", source.identity))
    {
        return Err("identity_mismatch");
    }
    let mut plist = Command::new("/usr/bin/plutil");
    plist
        .args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
        .arg(app.join("Contents/Info.plist"));
    if String::from_utf8_lossy(&run(plist, "identity_mismatch").await?.stdout).trim()
        != source.identity
    {
        return Err("identity_mismatch");
    }
    let mut trust = Command::new("/usr/sbin/spctl");
    trust.args(["--assess", "--type", "execute"]).arg(app);
    run(trust, "system_security_blocked").await?;
    Ok(())
}

#[cfg(target_os = "macos")]
pub(super) fn publish_exclusive(from: &Path, to: &Path) -> Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    unsafe extern "C" {
        fn renamex_np(
            from: *const std::ffi::c_char,
            to: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    let from = CString::new(from.as_os_str().as_bytes()).map_err(|_| "unsafe_destination")?;
    let to = CString::new(to.as_os_str().as_bytes()).map_err(|_| "unsafe_destination")?;
    // SAFETY: valid NUL-terminated paths; RENAME_EXCL is defined by Darwin SDK.
    if unsafe { renamex_np(from.as_ptr(), to.as_ptr(), 0x4) } == 0 {
        Ok(())
    } else if std::io::Error::last_os_error().kind() == std::io::ErrorKind::AlreadyExists {
        Err("installation_location_conflict")
    } else {
        Err("installation_failed")
    }
}

#[cfg(target_os = "macos")]
async fn mac_install(
    worker: &Worker,
    source: Source,
    folder: &Path,
    file: &Path,
) -> Result<Installed> {
    match source.extension {
        "zip" if source.tool == "claude_desktop" => {
            // Extraction is owned data processing, not an external extractor or
            // executable. Keep its guard and worker permit through copy/cleanup.
            let extracted = super::zip_package::extract(file, folder, || worker.check_cancelled())?;
            install_mac_app(worker, source, &extracted.app).await
        }
        "dmg" => mac_dmg_install(worker, source, folder, file).await,
        _ => Err("invalid_download"),
    }
}

#[cfg(target_os = "macos")]
async fn install_mac_app(worker: &Worker, source: Source, app: &Path) -> Result<Installed> {
    verify_app(source, app).await?;
    worker.check_cancelled()?; // Still no application mutation.
    if present(source).await? {
        return Err("already_installed");
    }
    let root = tool_adapters::user_home()
        .ok_or("home_unavailable")?
        .join("Applications");
    cache::directory(&root)?;
    let staging = cache::unique_dir(&root)?;
    let name = if source.tool == "codex_desktop" {
        "Codex.app"
    } else {
        "Claude.app"
    };
    let staged = staging.join(name);
    worker.phase("installing", false);
    // Never cancel/drop this mutation future. The worker retains admission
    // until copying, exclusive publication and owned package cleanup finish.
    let result = async {
        let mut copy = Command::new("/usr/bin/ditto");
        copy.args(["--rsrc", "--extattr"]).arg(app).arg(&staged);
        run(copy, "installation_failed").await?;
        verify_app(source, &staged).await?;
        if present(source).await? {
            return Err("already_installed");
        }
        let destination = root.join(name);
        publish_exclusive(&staged, &destination)?;
        Ok(Installed::Created(destination))
    }
    .await;
    // Only this create-exclusive private staging directory is owned here.
    let _ = std::fs::remove_dir_all(&staging);
    result
}

#[cfg(target_os = "macos")]
async fn mac_dmg_install(
    worker: &Worker,
    source: Source,
    folder: &Path,
    file: &Path,
) -> Result<Installed> {
    let mount = cache::unique_dir(folder)?;
    let mut verify = Command::new("/usr/bin/hdiutil");
    verify.arg("verify").arg(file);
    if let Err(error) = run(verify, "invalid_download").await {
        let _ = std::fs::remove_dir(&mount);
        return Err(error);
    }
    let mut attach = Command::new("/usr/bin/hdiutil");
    attach
        .args([
            "attach",
            "-readonly",
            "-nobrowse",
            "-noautoopen",
            "-mountpoint",
        ])
        .arg(&mount)
        .arg(file);
    // Detach is attempted even if attach timed out after mounting successfully.
    let mounted = run(attach, "mount_failed").await;
    let outcome = async {
        mounted?;
        let apps = std::fs::read_dir(&mount)
            .map_err(cache::io_error)?
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "app"))
            .collect::<Vec<_>>();
        if apps.len() != 1 || !apps[0].file_type().map_err(cache::io_error)?.is_dir() {
            return Err("identity_mismatch");
        }
        install_mac_app(worker, source, &apps[0].path()).await
    }
    .await;
    let mut detach = Command::new("/usr/bin/hdiutil");
    detach.arg("detach").arg(&mount);
    let detached = run(detach, "mount_cleanup_pending").await;
    if detached.is_ok() {
        let _ = std::fs::remove_dir(&mount);
    }
    if detached.is_err() && outcome.is_ok() {
        return Err("mount_cleanup_pending");
    }
    outcome
}

#[cfg(target_os = "windows")]
fn powershell() -> Result<Command> {
    let root = std::env::var_os("SystemRoot").ok_or("system_installer_unavailable")?;
    let path = PathBuf::from(root).join("System32/WindowsPowerShell/v1.0/powershell.exe");
    if !path.is_absolute() || !path.is_file() {
        return Err("system_installer_unavailable");
    }
    let mut command = Command::new(path);
    command.args(["-NoProfile", "-NonInteractive"]);
    use std::os::windows::process::CommandExt;
    command.as_std_mut().creation_flags(0x08000000);
    Ok(command)
}

#[cfg(any(target_os = "windows", test))]
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WindowsObservation {
    pub present: bool,
    pub path: String,
    #[serde(default)]
    pub reason: String,
}

#[cfg(target_os = "windows")]
async fn windows(
    source: Source,
    action: &str,
    file: Option<&Path>,
    hash: &str,
) -> Result<WindowsObservation> {
    let mut command = powershell()?;
    command
        .env("YESCHOY_INSTALL_ACTION", action)
        .env("YESCHOY_PACKAGE_NAME", source.identity)
        .env("YESCHOY_PACKAGE_PUBLISHER", source.publisher)
        .env("YESCHOY_PACKAGE_ARCH", source.architecture)
        .env("YESCHOY_PACKAGE_SHA256", hash)
        .args(["-Command", include_str!("windows.ps1")]);
    if let Some(file) = file {
        command.env("YESCHOY_PACKAGE_FILE", file);
    }
    let result = run(command, "system_installer_unavailable").await?;
    let observation: WindowsObservation =
        serde_json::from_slice(&result.stdout).map_err(|_| "presence_unconfirmed")?;
    match observation.reason.as_str() {
        "" => Ok(observation),
        "unsafe_cache" => Err("unsafe_cache"),
        "download_changed" => Err("download_changed"),
        "invalid_download" => Err("invalid_download"),
        "signature_invalid" => Err("signature_invalid"),
        "identity_mismatch" => Err("identity_mismatch"),
        "already_installed" => Err("already_installed"),
        "installation_unconfirmed" => Err("installation_unconfirmed"),
        _ => Err("system_installer_unavailable"),
    }
}
