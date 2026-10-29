//! Lifecycle/compensation ordering tests moved with the code they protect.
#![cfg(test)]

use super::*;
use std::time::Duration;

/// 这条测试的存在本身就是那个 bug 的纪念碑。
///
/// 判定原先写在 `if desktop_lifecycle::requires_reload(..)` 里面，而
/// `requires_reload` 只对 `claude_desktop` / `codex_desktop` 为真 ——
/// 里面再判 `claude_code`，两个互斥条件相与，永远为假。那段代码一次都没
/// 跑过，用户接入 Claude Code 后照旧看到
/// `Both claude.ai and apiKeyHelper set`，中转从头到尾没被用上。
///
/// 当时的测试直接 mock 了 `configure_desktop_tool_v2` 的返回，测的是界面
/// 契约，完全没经过这里 —— 绿着，而功能是死的。
#[test]
fn only_claude_code_without_consent_and_with_a_login_asks() {
    let present = || true;
    let absent = || false;

    assert!(needs_claude_login_consent("claude_code", false, present));

    // 已经同意过就不再问，否则每次接入都弹一次系统授权框。
    assert!(!needs_claude_login_consent("claude_code", true, present));
    // 没有 claude.ai 登录态的人不该平白收到任何东西。
    assert!(!needs_claude_login_consent("claude_code", false, absent));
    // 只有 Claude Code 会读 `~/.claude` 的凭据；别的工具不受影响。
    for tool in [
        "claude_desktop",
        "codex_desktop",
        "pi",
        "dsh_web",
        "workbuddy",
    ] {
        assert!(
            !needs_claude_login_consent(tool, false, present),
            "{tool} 不该被问"
        );
    }
}

/// 探测要 shell 出去查钥匙串。除了 claude_code 且尚未同意，谁都不该付这个
/// 代价 —— 每次接入都多跑一个进程是白花的。
#[test]
fn the_keychain_is_not_probed_when_the_answer_is_already_no() {
    let mut probed = false;
    let mut probe = || {
        probed = true;
        true
    };
    assert!(!needs_claude_login_consent("workbuddy", false, &mut probe));
    assert!(!probed, "别的工具不该触发探测");

    let mut probed = false;
    let mut probe = || {
        probed = true;
        true
    };
    assert!(!needs_claude_login_consent("claude_code", true, &mut probe));
    assert!(!probed, "已经同意过就不必再探测");
}

#[test]
fn restoration_errors_never_masquerade_as_initial_storage_failure() {
    assert_eq!(restoration_failure(false, false), None);
    assert_eq!(
        restoration_failure(true, false),
        Some(AdapterFailure::ConfigurationFailed(
            "configuration_rollback_failed"
        ))
    );
    assert_eq!(
        restoration_failure(false, true),
        Some(AdapterFailure::ConfigurationFailed(
            "credential_restore_failed"
        ))
    );
    assert_eq!(
        restoration_failure(true, true),
        Some(AdapterFailure::ConfigurationFailed(
            "configuration_rollback_failed"
        ))
    );
}

#[tokio::test]
async fn refused_stop_does_not_restore_files_or_keys_or_delete_live_tokens() {
    assert!(matches!(
        recover_after_stop(
            async { Err(AdapterFailure::LaunchError("graceful_restart_required")) },
            || async { panic!("must not change live files or revoke live keys") }
        )
        .await,
        Some(AdapterFailure::ConfigurationFailed(
            "desktop_recovery_waiting_for_exit"
        ))
    ));
    assert!(matches!(
        recover_after_stop(async { Ok(()) }, || async {
            Some(AdapterFailure::ConfigurationFailed(
                "credential_restore_failed",
            ))
        })
        .await,
        Some(AdapterFailure::ConfigurationFailed(
            "credential_restore_failed"
        ))
    ));
    assert!(recover_after_stop(async { Ok(()) }, || async { None })
        .await
        .is_none());
}

#[tokio::test]
async fn desktop_receipt_cannot_complete_before_startup_observation() {
    use std::cell::RefCell;
    for startup_ok in [false, true] {
        let events = RefCell::new(Vec::new());
        let result = finalize_after_start(
            Ok(()),
            async {
                events.borrow_mut().push("history-commit");
                Ok(())
            },
            async {
                events.borrow_mut().push("start-observed");
                if startup_ok {
                    Ok(())
                } else {
                    Err(AdapterFailure::LaunchError("desktop_start_unconfirmed"))
                }
            },
            || {
                events.borrow_mut().push("finish-receipt");
                Ok(())
            },
        )
        .await;
        assert_eq!(result.is_ok(), startup_ok);
        assert_eq!(
            *events.borrow(),
            if startup_ok {
                vec!["history-commit", "start-observed", "finish-receipt"]
            } else {
                vec!["history-commit", "start-observed"]
            }
        );
    }
    assert!(finalize_after_start(
        Err(AdapterFailure::ConfigurationFailed(
            "synthetic-write-failed"
        )),
        async { panic!("must not commit history after a failed configuration") },
        async { panic!("must not launch after a failed configuration") },
        || panic!("must not finish after a failed configuration")
    )
    .await
    .is_err());
    assert!(
        finalize_after_start(Ok(()), async { Ok(()) }, async { Ok(()) }, || Err(
            AdapterFailure::ConfigurationFailed("activation_cancelled")
        ))
        .await
        .is_err()
    );
}

#[tokio::test]
async fn codex_history_worker_does_not_block_async_runtime() {
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = std::sync::mpsc::channel();
    let task = tokio::spawn(codex_history_on_worker(move || {
        started_tx.send(()).unwrap();
        finish_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        Ok::<_, AdapterFailure>(7)
    }));
    started_rx.await.unwrap();
    tokio::time::timeout(Duration::from_millis(100), tokio::task::yield_now())
        .await
        .unwrap();
    finish_tx.send(()).unwrap();
    assert_eq!(task.await.unwrap().unwrap(), 7);
}
