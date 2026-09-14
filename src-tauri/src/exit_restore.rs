//! Process-owned user exit intent, separate from updater/installer shutdown.
use serde::Serialize;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExitStatus {
    Exiting,
    FinishingOperation,
    RestoringSettings,
    RestoreFailed,
    RetryableError,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExitResponse {
    pub(crate) status: ExitStatus,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) failed_tools: Vec<String>,
}

impl ExitResponse {
    pub(crate) fn new(status: ExitStatus) -> Self {
        Self {
            status,
            failed_tools: vec![],
        }
    }
}

#[derive(Default)]
pub(crate) struct ExitAttempt {
    running: bool,
    response: Option<ExitResponse>,
}

impl ExitAttempt {
    // Duplicate requests join the running attempt, never cancel its writes.
    pub(crate) fn begin(&mut self) -> bool {
        if self.running {
            return false;
        }
        self.running = true;
        self.response = Some(ExitResponse::new(ExitStatus::FinishingOperation));
        true
    }
    pub(crate) fn update(&mut self, response: ExitResponse) {
        self.response = Some(response);
    }
    pub(crate) fn finish(&mut self, response: ExitResponse) {
        self.running = false;
        self.update(response);
    }
    pub(crate) fn response(&self) -> Option<ExitResponse> {
        self.response.clone()
    }
}

pub(crate) fn attempt() -> &'static Mutex<ExitAttempt> {
    static ATTEMPT: OnceLock<Mutex<ExitAttempt>> = OnceLock::new();
    ATTEMPT.get_or_init(Mutex::default)
}

pub(crate) const TOOLS: [&str; 5] = [
    "claude_code",
    "claude_desktop",
    "codex_desktop",
    "pi",
    "dsh_web",
];

/// Continue other tools after a recoverable failure; callers retain journals.
pub(crate) async fn restore_all<F, Fut>(mut restore: F) -> Vec<String>
where
    F: FnMut(&'static str) -> Fut,
    Fut: std::future::Future<Output = Result<(), ()>>,
{
    let mut failed = vec![];
    for tool in TOOLS {
        if restore(tool).await.is_err() {
            failed.push(tool.to_owned());
        }
    }
    failed
}

/// A refused normal quit must prevent any configuration or credential write.
pub(crate) async fn after_normal_close<C, W, Fut>(close: C, write: W) -> Result<(), ()>
where
    C: std::future::Future<Output = Result<(), ()>>,
    W: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), ()>>,
{
    close.await?;
    write().await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exit_restore_duplicate_retry_and_readback() {
        let mut state = ExitAttempt::default();
        assert!(state.begin());
        assert!(!state.begin());
        state.update(ExitResponse::new(ExitStatus::RestoringSettings));
        assert_eq!(
            state.response().unwrap().status,
            ExitStatus::RestoringSettings
        );
        state.finish(ExitResponse {
            status: ExitStatus::RestoreFailed,
            failed_tools: vec!["pi".into()],
        });
        assert_eq!(state.response().unwrap().failed_tools, vec!["pi"]);
        assert!(state.begin());
        assert!(state.response().unwrap().failed_tools.is_empty());
    }
    #[tokio::test]
    async fn exit_restore_partial_failure_does_not_skip_other_tools() {
        let mut seen = vec![];
        let failed = restore_all(|tool| {
            seen.push(tool);
            async move {
                if tool == "codex_desktop" {
                    Err(())
                } else {
                    Ok(())
                }
            }
        })
        .await;
        assert_eq!(seen, TOOLS);
        assert_eq!(failed, vec!["codex_desktop"]);
        assert!(restore_all(|_| async { Ok(()) }).await.is_empty());
    }
    #[tokio::test]
    async fn exit_restore_refused_quit_never_writes() {
        assert!(after_normal_close(async { Err(()) }, || async {
            panic!("must not write while the application is still running")
        })
        .await
        .is_err());
        assert!(after_normal_close(async { Ok(()) }, || async { Ok(()) })
            .await
            .is_ok());
    }
    #[test]
    fn exit_restore_uses_conflict_preserving_original_and_is_idempotent() {
        use crate::{connection_recovery::restore_bytes, tool_adapters::common::FileTransaction};
        let original = br#"{"model":"original","theme":"light"}"#;
        let applied = br#"{"model":"yeschoy","theme":"light"}"#;
        let change = FileTransaction::stage_with_snapshot(
            std::env::temp_dir().join("exit-restore-fixture.json"),
            Some(original.to_vec()),
            applied.to_vec(),
        )
        .unwrap();
        let file = &change.changes()[0];
        let later = br#"{"model":"yeschoy","theme":"dark"}"#;
        let (restored, _) = restore_bytes(file, Some(later)).unwrap();
        let restored = restored.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&restored).unwrap();
        assert_eq!(parsed["model"], "original");
        assert_eq!(parsed["theme"], "dark");
        assert_eq!(
            restore_bytes(file, Some(&restored)).unwrap().0.unwrap(),
            restored
        );
        let user_model = br#"{"model":"user-choice","theme":"dark"}"#;
        let (preserved, kept) = restore_bytes(file, Some(user_model)).unwrap();
        assert!(kept);
        assert_eq!(preserved.unwrap(), user_model);
    }
    #[test]
    fn exit_restore_updater_and_native_windows_entries_remain_distinct() {
        let lib = include_str!("lib.rs");
        let drain = lib
            .split("pub(crate) async fn drain_desktop_runtimes")
            .nth(1)
            .unwrap()
            .split("async fn wait_desktop_operations")
            .next()
            .unwrap();
        assert!(!drain.contains("restore_connection"));
        assert!(drain.contains("stop_registered"));
        let updater = include_str!("app_update.rs");
        assert!(updater.contains("drain_desktop_runtimes(app).await"));
        assert!(!updater.contains("begin_user_shutdown"));
        assert!(lib.contains("begin_user_shutdown(window.app_handle().clone(), true)"));
        let lifecycle = include_str!("tool_adapters/desktop_lifecycle.rs");
        let normal = lifecycle
            .split("pub(crate) async fn quit_for_exit_restore")
            .nth(1)
            .unwrap()
            .split("/// Close the exact")
            .next()
            .unwrap();
        assert!(!normal.contains("blocking_force_quit("));
        assert!(normal.contains("blocking_normal_quit"));
    }
    #[test]
    fn exit_restore_fixture_binary_matches_current_sources() {
        // The isolated verifier may run this already-built test executable.
        // Fail closed if any source participating in the exit path changed
        // since compilation; do not substitute an old green test binary.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for (name, compiled) in [
            ("exit_restore.rs", include_str!("exit_restore.rs")),
            ("lib.rs", include_str!("lib.rs")),
            ("tool_activation.rs", include_str!("tool_activation.rs")),
            (
                "connection_recovery.rs",
                include_str!("connection_recovery.rs"),
            ),
            ("tool_credentials.rs", include_str!("tool_credentials.rs")),
            (
                "shutdown_coordinator.rs",
                include_str!("shutdown_coordinator.rs"),
            ),
            ("app_update.rs", include_str!("app_update.rs")),
            (
                "tool_adapters/desktop_lifecycle.rs",
                include_str!("tool_adapters/desktop_lifecycle.rs"),
            ),
        ] {
            assert_eq!(
                std::fs::read_to_string(root.join(name)).unwrap(),
                compiled,
                "{name}"
            );
        }
    }
}
