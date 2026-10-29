//! Activation use cases: request, tokens, capability evidence, lifecycle and
//! compensation, explicit-dependency service, admission session and IPC result.
//! The native root keeps only the Tauri command facade for these use cases.
pub(crate) mod capability;
#[cfg(test)]
#[path = "end_to_end_tests.rs"]
pub(crate) mod end_to_end_tests;
pub(crate) mod envelope;
pub(crate) mod error;
pub(crate) mod issuance_recovery;
pub(crate) mod lifecycle;
pub(crate) mod request;
pub(crate) mod result;
pub(crate) mod service;
pub(crate) mod session;
pub(crate) mod tokens;
#[cfg(test)]
pub(crate) mod tokens_tests_support;
