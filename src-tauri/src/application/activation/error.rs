//! Closed activation failures and their existing renderer projections. The
//! projection type stays in the facade so the IPC schema is unchanged.
pub(crate) use crate::domain::error::AdapterFailure;
use crate::tool_activation::ToolActivationProjection;

use super::request::ToolActivationRequest;

#[derive(Clone, Copy, Debug)]
pub(crate) enum ActivationFailure {
    SignedOut,
    /// Nothing produces these two any more. Both gates were withdrawn -- the
    /// model one in `1b3fb135`, the billing group one here -- because both
    /// judged a model unusable from an absent or unreliable declaration. The
    /// variants stay only so the renderer keeps its recovery copy for an older
    /// native binary; do not reach for them to add a gate back without a
    /// signal you can trust. See src/configuration/modelCompatibility.ts.
    UnsupportedModel,
    ServerUnavailable,
    Adapter(AdapterFailure),
    ConfigurationFailed(&'static str),
}

impl ActivationFailure {
    pub(crate) fn projection(self, request: &ToolActivationRequest) -> ToolActivationProjection {
        match self {
            Self::SignedOut => ToolActivationProjection::new(request, "signed_out", "signed_out"),
            Self::UnsupportedModel => {
                ToolActivationProjection::new(request, "unsupported_model", "model_not_available")
            }
            Self::ServerUnavailable => {
                ToolActivationProjection::new(request, "server_unavailable", "server_unavailable")
            }
            Self::Adapter(error) => match error {
                AdapterFailure::ToolNotFound => {
                    ToolActivationProjection::new(request, "tool_not_found", "tool_not_found")
                }
                AdapterFailure::MultipleInstallations => ToolActivationProjection::new(
                    request,
                    "multiple_installations",
                    "installation_selection_required",
                ),
                AdapterFailure::MissingRuntime => ToolActivationProjection::new(
                    request,
                    "missing_runtime",
                    "required_runtime_not_found",
                ),
                AdapterFailure::UnsupportedProfile => ToolActivationProjection::new(
                    request,
                    "unsupported_profile",
                    "profile_not_supported",
                ),
                AdapterFailure::ExternalOverride => ToolActivationProjection::new(
                    request,
                    "external_override",
                    "higher_precedence_override",
                ),
                AdapterFailure::SecureStorageUnavailable => ToolActivationProjection::new(
                    request,
                    "secure_storage_unavailable",
                    "secure_storage_unavailable",
                ),
                AdapterFailure::ConfigurationFailed(reason) => {
                    ToolActivationProjection::new(request, "configuration_failed", reason)
                }
                AdapterFailure::LaunchFailed => {
                    ToolActivationProjection::new(request, "launch_failed", "tool_launch_failed")
                }
                AdapterFailure::LaunchError(reason) => {
                    ToolActivationProjection::new(request, "launch_failed", reason)
                }
            },
            Self::ConfigurationFailed(reason) => {
                ToolActivationProjection::new(request, "configuration_failed", reason)
            }
        }
    }
}
