//! Connection inspection worker tests moved with the use case. They use lock
//! fixtures only and never read the real keychain or configuration.
#![cfg(test)]

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[tokio::test]
async fn inspection_regression_one_adapter_panic_keeps_the_other_results() {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let result = inspect_on_worker(&LOCK, Duration::from_secs(2), || {
        inspect_connections_with(|tool| {
            if tool == "codex_desktop" {
                panic!("synthetic-private-value-must-not-be-in-response");
            }
            let mut projection = ConnectionProjection::empty(tool);
            if tool == "claude_desktop" {
                projection.state = "connected";
                projection.reason_code = "connected";
            }
            projection
        })
    })
    .await
    .unwrap();
    assert_eq!(result.len(), CONNECTION_TOOLS.len());
    assert_eq!(result[1].state, "connected");
    assert_eq!(result[2].state, "unavailable");
    assert_eq!(result[2].reason_code, "connection_inspection_failed");
    assert_eq!(
        result.iter().filter(|c| c.state == "not_connected").count(),
        CONNECTION_TOOLS.len() - 2
    );
    assert!(!serde_json::to_string(&result)
        .unwrap()
        .contains("synthetic-private"));
    assert!(LOCK.try_lock().is_ok());
    let next = inspect_on_worker(&LOCK, Duration::from_secs(2), || {
        inspect_connections_with(ConnectionProjection::empty)
    })
    .await
    .unwrap();
    assert!(next.iter().all(|c| c.state == "not_connected"));
}

#[tokio::test(flavor = "current_thread")]
async fn ru076_inspection_worker_keeps_runtime_responsive_and_write_lock_until_read_finishes() {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = std::sync::mpsc::channel();
    let task = tokio::spawn(inspect_on_worker(
        &LOCK,
        Duration::from_secs(2),
        move || {
            started_tx.send(()).unwrap();
            finish_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            7
        },
    ));
    started_rx.await.unwrap();
    // If the synchronous reader ran on this current-thread executor, we
    // could not reach this assertion or send its completion signal.
    assert!(LOCK.try_lock().is_err());
    tokio::task::yield_now().await;
    finish_tx.send(()).unwrap();
    assert_eq!(task.await.unwrap(), Ok(7));
    assert!(LOCK.try_lock().is_ok());
}

#[tokio::test]
async fn ru076_panicking_reader_has_a_safe_failure_and_releases_the_lock() {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let result = inspect_on_worker(&LOCK, Duration::from_secs(2), || {
        panic!("synthetic private storage detail")
    })
    .await;
    assert_eq!(result, Err("connection_inspection_failed"));
    assert!(LOCK.try_lock().is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn ru076_timed_out_reader_does_not_release_write_lock_or_queue_another_os_read() {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = std::sync::mpsc::channel();
    let task = tokio::spawn(inspect_on_worker(
        &LOCK,
        Duration::from_millis(80),
        move || {
            started_tx.send(()).unwrap();
            finish_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        },
    ));
    started_rx.await.unwrap();
    assert_eq!(task.await.unwrap(), Err("connection_inspect_timed_out"));
    assert!(LOCK.try_lock().is_err());
    let invoked = Arc::new(AtomicBool::new(false));
    let worker_invoked = invoked.clone();
    assert_eq!(
        inspect_on_worker(&LOCK, Duration::from_millis(20), move || {
            worker_invoked.store(true, Ordering::SeqCst);
        })
        .await,
        Err("connection_inspect_timed_out")
    );
    assert!(!invoked.load(Ordering::SeqCst));
    finish_tx.send(()).unwrap();
    let _guard = tokio::time::timeout(Duration::from_secs(2), LOCK.lock())
        .await
        .unwrap();
}
