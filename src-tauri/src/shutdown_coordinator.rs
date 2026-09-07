//! Process-owned shutdown admission and cancellation. Cancelling a permit only
//! signals its owner: it never aborts a credential or configuration transaction.
use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
    time::Duration,
};

use tokio::{sync::watch, task::JoinSet, time::Instant};

// Reuse the account request bound and existing Codex runtime grace, as frozen
// in RU-042. The former is a progress notice, not permission to abort a write.
pub(crate) const FINISHING_NOTICE_AFTER: Duration = Duration::from_secs(20);
pub(crate) const RUNTIME_STOP_GRACE: Duration = Duration::from_secs(2);

type StopFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
type StopHook = Box<dyn FnOnce() -> StopFuture + Send + 'static>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ShuttingDown;

impl std::fmt::Display for ShuttingDown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("shutting_down")
    }
}

impl std::error::Error for ShuttingDown {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RegisterError {
    ShuttingDown,
    Duplicate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ShutdownProgress {
    Running,
    Draining,
    FinishingOperation,
    StoppingRuntimes,
    ExitRequested,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DrainOutcome {
    Quiescent,
    FinishingOperation,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ShutdownReport {
    pub(crate) finished: Vec<&'static str>,
    pub(crate) cancelled: Vec<&'static str>,
    pub(crate) failed: Vec<&'static str>,
    pub(crate) ready_to_exit: bool,
}

struct State {
    progress: ShutdownProgress,
    active: usize,
    close_requested: bool,
    stops: BTreeMap<&'static str, StopHook>,
    report: Option<ShutdownReport>,
}

struct Inner {
    state: Mutex<State>,
    cancellation: watch::Sender<bool>,
    active: watch::Sender<usize>,
}

impl Inner {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
}

#[derive(Clone)]
pub(crate) struct ShutdownCoordinator {
    inner: Arc<Inner>,
}

impl Default for ShutdownCoordinator {
    fn default() -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    progress: ShutdownProgress::Running,
                    active: 0,
                    close_requested: false,
                    stops: BTreeMap::new(),
                    report: None,
                }),
                cancellation: watch::channel(false).0,
                active: watch::channel(0).0,
            }),
        }
    }
}

pub(crate) fn global() -> &'static ShutdownCoordinator {
    static COORDINATOR: OnceLock<ShutdownCoordinator> = OnceLock::new();
    COORDINATOR.get_or_init(ShutdownCoordinator::default)
}

#[must_use = "keep this permit until local transaction cleanup is safe"]
pub(crate) struct OperationPermit {
    inner: Arc<Inner>,
}

impl OperationPermit {
    pub(crate) fn is_cancelled(&self) -> bool {
        *self.inner.cancellation.borrow()
    }

    pub(crate) async fn cancelled(&self) {
        let mut signal = self.inner.cancellation.subscribe();
        // wait_for observes the current value as well as future changes, so a
        // shutdown between admission and this subscription cannot be lost.
        let _ = signal.wait_for(|requested| *requested).await;
    }

    /// Only for cancellation-safe work. Never wrap an entire transaction or a
    /// token-create/rotating-refresh request in this helper.
    pub(crate) async fn cancel_safe<F: Future>(&self, step: F) -> Result<F::Output, ShuttingDown> {
        tokio::select! {
            biased;
            _ = self.cancelled() => Err(ShuttingDown),
            result = step => Ok(result),
        }
    }
}

impl Drop for OperationPermit {
    fn drop(&mut self) {
        let mut state = self.inner.state();
        state.active -= 1;
        self.inner.active.send_replace(state.active);
    }
}

impl ShutdownCoordinator {
    pub(crate) fn admit_operation(&self) -> Result<OperationPermit, ShuttingDown> {
        let mut state = self.inner.state();
        if state.progress != ShutdownProgress::Running {
            return Err(ShuttingDown);
        }
        state.active += 1;
        self.inner.active.send_replace(state.active);
        Ok(OperationPermit {
            inner: self.inner.clone(),
        })
    }

    pub(crate) fn is_shutting_down(&self) -> bool {
        self.progress() != ShutdownProgress::Running
    }

    pub(crate) fn progress(&self) -> ShutdownProgress {
        self.inner.state().progress
    }

    pub(crate) fn request_close_choice(&self) {
        self.inner.state().close_requested = true;
    }

    pub(crate) fn close_choice_requested(&self) -> bool {
        self.inner.state().close_requested
    }

    pub(crate) fn dismiss_close_choice(&self) {
        self.inner.state().close_requested = false;
    }

    /// Returns true only for the owner of the one shutdown task.
    pub(crate) fn request_shutdown(&self) -> bool {
        let mut state = self.inner.state();
        if state.progress != ShutdownProgress::Running {
            return false;
        }
        state.progress = ShutdownProgress::Draining;
        state.close_requested = false;
        self.inner.cancellation.send_replace(true);
        true
    }

    pub(crate) async fn wait_quiescent(&self, deadline: Instant) -> DrainOutcome {
        let mut active = self.inner.active.subscribe();
        if tokio::time::timeout_at(deadline, active.wait_for(|count| *count == 0))
            .await
            .is_ok()
        {
            DrainOutcome::Quiescent
        } else {
            let mut state = self.inner.state();
            if matches!(
                state.progress,
                ShutdownProgress::Draining | ShutdownProgress::FinishingOperation
            ) {
                state.progress = ShutdownProgress::FinishingOperation;
            }
            DrainOutcome::FinishingOperation
        }
    }

    pub(crate) fn register_stop<F, Fut>(
        &self,
        id: &'static str,
        stop: F,
    ) -> Result<(), RegisterError>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let mut state = self.inner.state();
        if state.progress != ShutdownProgress::Running {
            return Err(RegisterError::ShuttingDown);
        }
        if state.stops.contains_key(id) {
            return Err(RegisterError::Duplicate);
        }
        state.stops.insert(id, Box::new(|| Box::pin(stop())));
        Ok(())
    }

    pub(crate) async fn stop_registered(&self, deadline: Instant) -> ShutdownReport {
        let stops = {
            let mut state = self.inner.state();
            if let Some(report) = &state.report {
                return report.clone();
            }
            if state.active != 0
                || !matches!(
                    state.progress,
                    ShutdownProgress::Draining | ShutdownProgress::FinishingOperation
                )
            {
                return ShutdownReport::default();
            }
            state.progress = ShutdownProgress::StoppingRuntimes;
            std::mem::take(&mut state.stops)
        };
        let mut tasks = JoinSet::new();
        let mut pending = HashMap::new();
        for (id, stop) in stops {
            let task = tasks.spawn(async move {
                stop().await;
                id
            });
            pending.insert(task.id(), id);
        }
        let mut report = ShutdownReport::default();
        while !pending.is_empty() {
            match tokio::time::timeout_at(deadline, tasks.join_next_with_id()).await {
                Ok(Some(Ok((task, id)))) => {
                    pending.remove(&task);
                    report.finished.push(id);
                }
                Ok(Some(Err(error))) => {
                    if let Some(id) = pending.remove(&error.id()) {
                        report.failed.push(id);
                    }
                }
                _ => {
                    report.cancelled.extend(pending.values().copied());
                    tasks.abort_all();
                    break;
                }
            }
        }
        // Aborting a shutdown future never targets a third-party desktop app.
        // Runtime stop implementations own their own tasks/children. Do not
        // add an unbounded join after the shared grace has expired.
        drop(tasks);
        report.finished.sort_unstable();
        report.cancelled.sort_unstable();
        report.failed.sort_unstable();
        report.ready_to_exit = true;
        let mut state = self.inner.state();
        state.progress = ShutdownProgress::ExitRequested;
        state.report = Some(report.clone());
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn close_choice_and_background_do_not_cancel_or_reject_operations() {
        let coordinator = ShutdownCoordinator::default();
        coordinator.request_close_choice();
        assert!(coordinator.close_choice_requested());
        let permit = coordinator.admit_operation().unwrap();
        coordinator.dismiss_close_choice();
        assert!(!coordinator.close_choice_requested());
        assert!(!permit.is_cancelled());
        assert!(!coordinator.is_shutting_down());
        assert!(coordinator.admit_operation().is_ok());
    }

    #[test]
    fn concurrent_admission_cannot_cross_the_confirmed_shutdown_gate() {
        let coordinator = ShutdownCoordinator::default();
        let start = Arc::new(std::sync::Barrier::new(9));
        let confirmed = Arc::new(std::sync::Barrier::new(9));
        let mut workers = Vec::new();
        for _ in 0..8 {
            let coordinator = coordinator.clone();
            let start = start.clone();
            let confirmed = confirmed.clone();
            workers.push(std::thread::spawn(move || {
                start.wait();
                let admitted = coordinator.admit_operation();
                confirmed.wait();
                if let Ok(permit) = admitted {
                    assert!(permit.is_cancelled());
                }
                assert!(matches!(coordinator.admit_operation(), Err(ShuttingDown)));
            }));
        }
        start.wait();
        assert!(coordinator.request_shutdown());
        confirmed.wait();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(coordinator.inner.state().active, 0);
    }

    #[tokio::test]
    async fn confirmation_rejects_new_operations_and_cancels_safe_waits() {
        let coordinator = ShutdownCoordinator::default();
        let permit = coordinator.admit_operation().unwrap();
        assert!(coordinator.request_shutdown());
        assert!(!coordinator.request_shutdown());
        assert!(matches!(coordinator.admit_operation(), Err(ShuttingDown)));
        assert_eq!(
            permit.cancel_safe(std::future::pending::<()>()).await,
            Err(ShuttingDown)
        );
        assert!(permit.is_cancelled());
        drop(permit);
        assert_eq!(
            coordinator.wait_quiescent(Instant::now()).await,
            DrainOutcome::Quiescent
        );
    }

    #[tokio::test]
    async fn safe_wait_is_cancelled_when_confirmation_arrives_after_subscription() {
        let coordinator = ShutdownCoordinator::default();
        let permit = coordinator.admit_operation().unwrap();
        let task =
            tokio::spawn(async move { permit.cancel_safe(std::future::pending::<()>()).await });
        tokio::task::yield_now().await;
        coordinator.request_shutdown();
        assert_eq!(task.await.unwrap(), Err(ShuttingDown));
    }

    #[tokio::test]
    async fn safe_ready_step_is_not_started_after_cancellation() {
        let coordinator = ShutdownCoordinator::default();
        let permit = coordinator.admit_operation().unwrap();
        coordinator.request_shutdown();
        let polled = AtomicBool::new(false);
        assert_eq!(
            permit
                .cancel_safe(async {
                    polled.store(true, Ordering::SeqCst);
                })
                .await,
            Err(ShuttingDown)
        );
        assert!(!polled.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn expired_notice_keeps_the_transaction_alive_until_safe_cleanup() {
        let coordinator = ShutdownCoordinator::default();
        let permit = coordinator.admit_operation().unwrap();
        coordinator.request_shutdown();
        assert_eq!(
            coordinator.wait_quiescent(Instant::now()).await,
            DrainOutcome::FinishingOperation
        );
        assert_eq!(coordinator.progress(), ShutdownProgress::FinishingOperation);
        assert!(
            !coordinator
                .stop_registered(Instant::now())
                .await
                .ready_to_exit
        );
        // An in-flight token mutation/local rollback still owns its permit.
        // Neither progress timeout nor repeated quit can drop it on its behalf.
        assert!(permit.is_cancelled());
        assert!(!coordinator.request_shutdown());
        drop(permit);
        assert_eq!(
            coordinator.wait_quiescent(Instant::now()).await,
            DrainOutcome::Quiescent
        );
        assert!(
            coordinator
                .stop_registered(Instant::now())
                .await
                .ready_to_exit
        );
    }

    #[tokio::test]
    async fn protected_mutation_and_local_cleanup_finish_before_runtime_stop() {
        let coordinator = ShutdownCoordinator::default();
        let permit = coordinator.admit_operation().unwrap();
        let cleaned = Arc::new(AtomicBool::new(false));
        let observed_cleanup = cleaned.clone();
        let (finish_mutation, mutation) = tokio::sync::oneshot::channel();
        let operation = tokio::spawn(async move {
            let _permit = permit;
            // Stand-in for an already-issued, non-cancellation-safe account
            // mutation. It is deliberately not passed to cancel_safe.
            mutation.await.unwrap();
            observed_cleanup.store(true, Ordering::SeqCst);
        });
        let observed_cleanup = cleaned.clone();
        coordinator
            .register_stop("after_cleanup", move || async move {
                assert!(observed_cleanup.load(Ordering::SeqCst));
            })
            .unwrap();
        coordinator.request_shutdown();
        assert_eq!(
            coordinator.wait_quiescent(Instant::now()).await,
            DrainOutcome::FinishingOperation
        );
        assert!(!operation.is_finished());
        assert!(!cleaned.load(Ordering::SeqCst));
        finish_mutation.send(()).unwrap();
        operation.await.unwrap();
        assert_eq!(
            coordinator.wait_quiescent(Instant::now()).await,
            DrainOutcome::Quiescent
        );
        let report = coordinator
            .stop_registered(Instant::now() + RUNTIME_STOP_GRACE)
            .await;
        assert_eq!(report.finished, vec!["after_cleanup"]);
        assert!(report.ready_to_exit);
    }

    #[tokio::test]
    async fn runtime_hooks_run_once_after_quiescence_without_the_state_lock() {
        let coordinator = ShutdownCoordinator::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let hook_coordinator = coordinator.clone();
        coordinator
            .register_stop("chat_gateway", move || async move {
                assert!(hook_coordinator.is_shutting_down());
                observed.fetch_add(1, Ordering::SeqCst);
            })
            .unwrap();
        assert_eq!(
            coordinator.register_stop("chat_gateway", || async {}),
            Err(RegisterError::Duplicate)
        );
        assert!(
            !coordinator
                .stop_registered(Instant::now())
                .await
                .ready_to_exit
        );
        coordinator.request_shutdown();
        assert_eq!(
            coordinator.register_stop("late", || async {}),
            Err(RegisterError::ShuttingDown)
        );
        let report = coordinator
            .stop_registered(Instant::now() + RUNTIME_STOP_GRACE)
            .await;
        assert_eq!(report.finished, vec!["chat_gateway"]);
        assert!(report.ready_to_exit);
        assert_eq!(coordinator.stop_registered(Instant::now()).await, report);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn stalled_runtime_hooks_share_one_deadline_and_are_cancelled() {
        let coordinator = ShutdownCoordinator::default();
        let dropped = Arc::new(AtomicUsize::new(0));
        struct OnDrop(Arc<AtomicUsize>);
        impl Drop for OnDrop {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        for id in ["first", "second"] {
            let drop_probe = dropped.clone();
            coordinator
                .register_stop(id, move || async move {
                    let _probe = OnDrop(drop_probe);
                    std::future::pending::<()>().await;
                })
                .unwrap();
        }
        coordinator.request_shutdown();
        let report = coordinator
            .stop_registered(Instant::now() + Duration::from_millis(10))
            .await;
        tokio::task::yield_now().await;
        assert_eq!(report.cancelled, vec!["first", "second"]);
        assert!(report.ready_to_exit);
        assert_eq!(dropped.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn process_global_accessor_is_one_instance() {
        // Do not mutate the global singleton from tests: independent fixtures
        // above deliberately use isolated coordinator instances.
        assert!(std::ptr::eq(global(), global()));
        assert_eq!(FINISHING_NOTICE_AFTER, Duration::from_secs(20));
        assert_eq!(RUNTIME_STOP_GRACE, Duration::from_secs(2));
    }
}
