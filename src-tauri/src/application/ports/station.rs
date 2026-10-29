//! Minimal station capability currently exercised by native activation.
//!
//! The application owns token enrollment/reuse/rollback policy. Providers own
//! the account protocol and HTTP details. This intentionally does not claim that
//! unknown providers implement New API endpoints or invent additional backends.

use std::future::Future;

use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StationMethod {
    Get,
    Post,
    Delete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StationProviderFailure {
    InvalidOrigin,
    InvalidPath,
    Transport,
}

pub(crate) trait StationProvider {
    /// Account-management JSON only, never a paid inference/test request.
    /// `path` is a same-origin `/api/` relative path, optionally with a query;
    /// it cannot be an absolute URL, authority, fragment, or traversal path.
    /// Providers return HTTP status and the existing envelope unchanged, so the
    /// application preserves its current 401/403 and server-rejection policies.
    /// Possessing a provider does not grant user trust; the caller must enforce
    /// the authorized native session and its epoch before account operations.
    fn account_json(
        &self,
        method: StationMethod,
        path: &str,
        body: Option<Value>,
    ) -> impl Future<Output = Result<(u16, Value), StationProviderFailure>> + Send;
}
