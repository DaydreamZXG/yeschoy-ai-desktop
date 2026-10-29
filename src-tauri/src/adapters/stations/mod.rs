//! Station protocols implemented by the current native backend.

mod new_api;

// Used by activation's token and capability steps; no allowance needed because
// those callers exercise every method the provider exposes.
pub(crate) use new_api::NewApiStationProvider;
