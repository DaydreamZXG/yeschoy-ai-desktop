#![cfg(test)]
//! In-process admission tests moved with the session state they protect.
use super::*;

#[test]
fn activation_cancellation_allows_only_one_live_transaction() {
    let state = ActivationOperationState::default();
    let registration = state.begin("activation-one").unwrap();
    assert!(state.begin("activation-one").is_err());
    assert!(state.begin("activation-two").is_err());
    assert!(!registration.cancellation.is_requested());
    assert_eq!(state.cancel("activation-one"), Ok(true));
    assert!(registration.cancellation.is_requested());
    drop(registration);
    assert_eq!(state.cancel("activation-one"), Ok(false));
}
