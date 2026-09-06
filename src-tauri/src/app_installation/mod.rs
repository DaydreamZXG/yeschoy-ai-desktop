mod cache;
mod catalog;
mod download;
mod origins;
mod platform;
#[cfg(test)]
mod tests;
#[cfg(any(target_os = "macos", all(test, unix)))]
mod zip_package;

use crate::{
    account_v2::{ensure_session_epoch, native_session_epoch, AccountV2State, NativeSessionEpoch},
    shutdown_coordinator::{self, OperationPermit},
    tool_activation::ModelBinding,
    tool_adapters,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::Manager;
use tokio::sync::watch;

type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Intent {
    pub(crate) line_id: String,
    pub(crate) model_id: String,
    pub(crate) billing_group: String,
    pub(crate) models: Vec<ModelBinding>,
}
impl Intent {
    fn valid(&self) -> bool {
        let text = |s: &str, max| {
            !s.is_empty()
                && s.len() <= max
                && !s.chars().any(|c| {
                    c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
                })
        };
        let mut ids = std::collections::HashSet::new();
        matches!(
            self.line_id.as_str(),
            "mainland_optimized" | "global_accelerated"
        ) && text(&self.model_id, 200)
            && text(&self.billing_group, 128)
            && self.billing_group != "auto"
            && !self.models.is_empty()
            && self.models.len() <= 200
            && self.models.iter().all(|m| {
                text(&m.model_id, 200)
                    && text(&m.billing_group, 128)
                    && m.billing_group != "auto"
                    && ids.insert(&m.model_id)
            })
            && self
                .models
                .iter()
                .any(|m| m.model_id == self.model_id && m.billing_group == self.billing_group)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallRequest {
    request_id: String,
    tool_id: String,
    action: String,
    job_id: String,
    #[serde(default)]
    intent: Option<Intent>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProjection {
    schema_version: u8,
    request_id: String,
    tool_id: String,
    job_id: String,
    mode: &'static str,
    phase: &'static str,
    downloaded_bytes: u64,
    total_bytes: u64,
    can_cancel: bool,
    reason_code: &'static str,
    source: &'static str,
    platform: &'static str,
    architecture: &'static str,
    disposition: &'static str,
    installation_id: String,
}
impl InstallProjection {
    fn idle(request: &InstallRequest) -> Self {
        let os = catalog::platform();
        let arch = catalog::architecture();
        Self {
            schema_version: 2,
            request_id: request.request_id.clone(),
            tool_id: request.tool_id.clone(),
            job_id: String::new(),
            mode: catalog::mode(&request.tool_id, os, arch),
            phase: "idle",
            downloaded_bytes: 0,
            total_bytes: 0,
            can_cancel: false,
            reason_code: "none",
            source: "none",
            platform: os,
            architecture: arch,
            disposition: "none",
            installation_id: String::new(),
        }
    }
    fn active(&self) -> bool {
        matches!(
            self.phase,
            "checking"
                | "downloading"
                | "verifying"
                | "installing"
                | "checking_install"
                | "awaiting_system_confirmation"
        )
    }
}

struct Consent {
    intent: Intent,
    epoch: NativeSessionEpoch,
}
struct Job {
    progress: InstallProjection,
    start_request_id: String,
    cancel: watch::Sender<bool>,
    consent: Option<Consent>,
    package: Option<(PathBuf, String)>,
    operating: bool,
}
impl Job {
    fn cancel_guidance(&mut self) {
        self.consent = None;
        if self.progress.can_cancel {
            self.cancel.send_replace(true);
            self.progress.can_cancel = false;
        } else if !self.operating && self.progress.phase == "awaiting_system_confirmation" {
            self.progress.phase = "cancelled";
            self.progress.reason_code = "system_handoff_dismissed";
        }
    }
    fn blocks_start(&self, request_id: &str) -> bool {
        self.progress.active() || self.start_request_id == request_id
    }
}

#[derive(Clone, Default)]
pub struct AppInstallationState(Arc<Mutex<Option<Job>>>);
impl AppInstallationState {
    fn read(&self, request: &InstallRequest) -> InstallProjection {
        let state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(job) = state.as_ref().filter(|job| {
            request.job_id == job.progress.job_id
                || (request.job_id.is_empty()
                    && (job.progress.active() || job.progress.tool_id == request.tool_id))
        }) {
            let mut result = job.progress.clone();
            result.request_id = request.request_id.clone();
            result
        } else {
            InstallProjection::idle(request)
        }
    }
    pub(crate) fn claim(
        &self,
        id: &str,
        tool: &str,
        installation: &str,
        intent: &Intent,
        account: &AccountV2State,
    ) -> Result<NativeSessionEpoch> {
        self.claim_checked(id, tool, installation, intent, |epoch| {
            ensure_session_epoch(account, epoch).map_err(|_| "installation_confirmation_required")
        })
    }
    fn claim_checked(
        &self,
        id: &str,
        tool: &str,
        installation: &str,
        intent: &Intent,
        validate_epoch: impl FnOnce(NativeSessionEpoch) -> Result<()>,
    ) -> Result<NativeSessionEpoch> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| "installation_confirmation_required")?;
        let job = state
            .as_mut()
            .filter(|j| j.progress.job_id == id)
            .ok_or("installation_confirmation_required")?;
        // Taking first makes stale/mismatched proofs non-replayable as well.
        let consent = job
            .consent
            .take()
            .ok_or("installation_confirmation_required")?;
        if job.progress.phase != "installed"
            || !matches!(job.progress.disposition, "created" | "confirmed")
            || job.progress.tool_id != tool
            || job.progress.installation_id != installation
            || consent.intent != *intent
        {
            return Err("installation_confirmation_required");
        }
        validate_epoch(consent.epoch)?;
        Ok(consent.epoch)
    }
}

struct Worker {
    state: AppInstallationState,
    id: String,
    cancel: watch::Receiver<bool>,
    permit: OperationPermit,
}
enum Work {
    Start(PathBuf),
    Confirm(PathBuf, String),
    Reopen(PathBuf, String),
    #[cfg(test)]
    FailedWorker,
}
impl Worker {
    fn launch(self, source: catalog::Source, work: Work) {
        let supervisor = self.state.clone();
        let id = self.id.clone();
        // IPC may disappear after handoff. Native ownership and its shutdown
        // permit outlive the caller, including confirm/reopen and cleanup.
        tokio::spawn(async move {
            let handle = tokio::spawn(async move {
                let result = async {
                    self.check_cancelled()?;
                    match work {
                        #[cfg(test)]
                        Work::FailedWorker => panic!("isolated installer worker fixture"),
                        Work::Start(folder) => self.execute(source, folder).await,
                        Work::Confirm(file, hash) => {
                            self.phase("checking_install", false);
                            let path = platform::confirmed(source, &file, &hash).await?;
                            self.associate(source.tool, &path, "confirmed").await
                        }
                        Work::Reopen(file, hash) => platform::install(
                            &self,
                            source,
                            file.parent().ok_or("unsafe_cache")?,
                            &file,
                            &hash,
                        )
                        .await
                        .map(|_| ()),
                    }
                }
                .await;
                self.finish(result);
            });
            if handle.await.is_err() {
                let mut guard = supervisor.0.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(job) = guard.as_mut().filter(|j| j.progress.job_id == id) {
                    job.operating = false;
                    job.consent = None;
                    job.progress.phase = "failed";
                    job.progress.can_cancel = false;
                    job.progress.reason_code = "installer_interrupted";
                }
            }
        });
    }
    fn update(&self, f: impl FnOnce(&mut Job)) {
        let mut state = self.state.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(job) = state.as_mut().filter(|j| j.progress.job_id == self.id) {
            f(job);
        }
    }
    fn phase(&self, phase: &'static str, cancel: bool) {
        self.update(|job| {
            job.progress.phase = phase;
            job.progress.can_cancel = cancel;
        });
    }
    fn bytes(&self, received: u64, total: u64) {
        self.update(|job| {
            job.progress.downloaded_bytes = received;
            job.progress.total_bytes = total;
        });
    }
    fn check_cancelled(&self) -> Result<()> {
        if *self.cancel.borrow() || self.permit.is_cancelled() {
            Err("cancelled")
        } else {
            Ok(())
        }
    }
    async fn fetch(&self, source: catalog::Source, folder: &Path) -> Result<(PathBuf, String)> {
        self.cancellable_download(download::fetch(self, source, folder))
            .await
    }
    async fn cancellable_download(
        &self,
        download: impl std::future::Future<Output = Result<(PathBuf, String)>>,
    ) -> Result<(PathBuf, String)> {
        let mut cancel = self.cancel.clone();
        self.check_cancelled()?;
        tokio::select! {
            _ = self.permit.cancelled() => Err("cancelled"),
            _ = cancel.changed() => Err("cancelled"),
            result = tokio::time::timeout(Duration::from_secs(1800), download) => result.unwrap_or(Err("download_timeout")),
        }
    }
    async fn associate(&self, tool: &str, path: &Path, disposition: &'static str) -> Result<()> {
        self.phase("checking_install", false);
        let targets = tool_adapters::scan_targets().await;
        let target = targets
            .iter()
            .find(|t| t.tool_id == tool)
            .ok_or("installation_unconfirmed")?;
        if target.installations.len() != 1 || !target.installations[0].supported {
            return Err("installation_unconfirmed");
        }
        let id = &target.installations[0].installation_id;
        let resolved = tool_adapters::resolve_installation(tool, id)
            .await
            .map_err(|_| "installation_unconfirmed")?;
        if std::fs::canonicalize(&resolved.path).ok() != std::fs::canonicalize(path).ok()
            || !path.exists()
        {
            return Err("installation_unconfirmed");
        }
        self.update(|job| {
            job.progress.phase = "installed";
            job.progress.disposition = disposition;
            job.progress.installation_id = id.clone();
            job.progress.reason_code = "none";
        });
        Ok(())
    }
    async fn execute(&self, source: catalog::Source, folder: PathBuf) -> Result<()> {
        self.check_cancelled()?;
        if platform::present(source).await? {
            return Err("already_installed");
        }
        self.phase("downloading", true);
        let (file, hash) = self.fetch(source, &folder).await?;
        self.check_cancelled()?;
        self.phase("verifying", false);
        self.update(|j| j.package = Some((file.clone(), hash.clone())));
        // Do not put installation/cleanup inside a cancelling select/timeout.
        let installed = platform::install(self, source, &folder, &file, &hash).await;
        if matches!(
            installed,
            Err("signature_invalid"
                | "identity_mismatch"
                | "invalid_download"
                | "download_changed")
        ) {
            // Rejecting package identity must also invalidate its resumable
            // generation, so Retry cannot repeatedly reuse that same bad file.
            cache::write(&folder.join("download.json"), b"{}")?;
        }
        match installed? {
            #[cfg(target_os = "macos")]
            platform::Installed::Created(path) => {
                self.associate(source.tool, &path, "created").await
            }
            #[cfg(target_os = "windows")]
            platform::Installed::SystemConfirmation => Ok(()),
        }
    }
    fn finish(&self, result: Result<()>) {
        self.update(|job| {
            job.operating = false;
            job.progress.can_cancel = false;
            if let Err(reason) = result {
                job.progress.reason_code = reason;
                job.consent = None;
                if reason == "already_installed" {
                    job.progress.phase = "installed";
                    job.progress.disposition = "existing";
                } else if reason == "installation_unconfirmed" || reason == "mount_cleanup_pending"
                {
                    job.progress.phase = "installed";
                    job.progress.disposition = "unconfirmed";
                } else if reason == "installation_not_detected" {
                    job.progress.phase = "awaiting_system_confirmation";
                } else {
                    job.progress.phase = if reason == "cancelled" {
                        "cancelled"
                    } else {
                        "failed"
                    };
                }
            }
        });
    }
}

fn new_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| "installer_unavailable")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[tauri::command]
pub async fn manage_app_installation_v2(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppInstallationState>,
    account: tauri::State<'_, AccountV2State>,
    request: InstallRequest,
) -> std::result::Result<InstallProjection, String> {
    control(app, &state, &account, request)
        .await
        .map_err(str::to_owned)
}

async fn control(
    app: tauri::AppHandle,
    state: &AppInstallationState,
    account: &AccountV2State,
    request: InstallRequest,
) -> Result<InstallProjection> {
    let valid_id = |s: &str| {
        s.len() <= 64
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    };
    if request.request_id.is_empty()
        || !valid_id(&request.request_id)
        || !valid_id(&request.job_id)
        || catalog::guide(&request.tool_id).is_none()
        || request.intent.as_ref().is_some_and(|i| !i.valid())
    {
        return Err("invalid_installation_request");
    }
    match request.action.as_str() {
        "inspect" => return Ok(state.read(&request)),
        "help" => {
            platform::open_guide(
                catalog::guide(&request.tool_id).ok_or("invalid_installation_request")?,
            )
            .await?;
            return Ok(state.read(&request));
        }
        "cancel" => {
            let mut guard = state.0.lock().map_err(|_| "installer_unavailable")?;
            let job = guard
                .as_mut()
                .filter(|j| {
                    j.progress.job_id == request.job_id && j.progress.tool_id == request.tool_id
                })
                .ok_or("installation_job_missing")?;
            // Explicit cancellation always revokes auto-connection permission,
            // even while a non-cancellable OS operation finishes safely.
            job.cancel_guidance();
            drop(guard);
            return Ok(state.read(&request));
        }
        "start" | "confirm" | "reopen" => (),
        _ => return Err("invalid_installation_request"),
    }
    let plan = InstallProjection::idle(&request);
    let Some(source) = catalog::source(&request.tool_id, plan.platform, plan.architecture) else {
        return Ok(plan);
    };
    let permit = shutdown_coordinator::global()
        .admit_operation()
        .map_err(|_| "assistant_shutting_down")?;
    let intent = request
        .intent
        .clone()
        .map(|intent| native_session_epoch(account).map(|epoch| Consent { intent, epoch }))
        .transpose()
        .map_err(|_| "installer_unavailable")?;
    if request.action != "start" {
        let (id, receiver, package) = {
            let mut guard = state.0.lock().map_err(|_| "installer_unavailable")?;
            let job = guard
                .as_mut()
                .filter(|j| {
                    j.progress.job_id == request.job_id && j.progress.tool_id == request.tool_id
                })
                .ok_or("installation_job_missing")?;
            if job.operating || job.progress.phase != "awaiting_system_confirmation" {
                drop(guard);
                return Ok(state.read(&request));
            }
            let package = job.package.clone().ok_or("installation_job_missing")?;
            job.operating = true;
            job.consent = intent;
            job.progress.reason_code = "none";
            (job.progress.job_id.clone(), job.cancel.subscribe(), package)
        };
        let worker = Worker {
            state: state.clone(),
            id,
            cancel: receiver,
            permit,
        };
        let work = if request.action == "confirm" {
            Work::Confirm(package.0, package.1)
        } else {
            Work::Reopen(package.0, package.1)
        };
        worker.launch(source, work);
        return Ok(state.read(&request));
    }
    if !request.job_id.is_empty() {
        return Err("invalid_installation_request");
    }
    let cache_root = app
        .path()
        .app_cache_dir()
        .map_err(|_| "cache_unavailable")?;
    if !cache_root.exists() {
        std::fs::create_dir_all(&cache_root).map_err(cache::io_error)?;
    }
    cache::directory(&cache_root)?;
    let cache_root = cache_root.join("installers-v2");
    cache::directory(&cache_root)?;
    let folder = cache_root.join(format!(
        "{}-{}-{}",
        source.tool, plan.platform, source.architecture
    ));
    let (id, receiver) = {
        let mut guard = state.0.lock().map_err(|_| "installer_unavailable")?;
        if guard
            .as_ref()
            .is_some_and(|j| j.blocks_start(&request.request_id))
        {
            drop(guard);
            return Ok(state.read(&request));
        }
        let id = new_id()?;
        let (sender, receiver) = watch::channel(false);
        let mut progress = plan;
        progress.job_id = id.clone();
        progress.phase = "checking";
        progress.can_cancel = true;
        *guard = Some(Job {
            progress,
            start_request_id: request.request_id.clone(),
            cancel: sender,
            consent: intent,
            package: None,
            operating: true,
        });
        (id, receiver)
    };
    let worker = Worker {
        state: state.clone(),
        id,
        cancel: receiver,
        permit,
    };
    worker.launch(source, Work::Start(folder));
    Ok(state.read(&request))
}
