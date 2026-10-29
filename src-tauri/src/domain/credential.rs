//! What a tool is handed once the relay has issued its key.
//!
//! Lives in `domain` because both sides of the dependency need it and neither
//! should own the other: the credential *store* (keyring) reads and writes this
//! shape, and connection *recovery* inspects it to decide which values it may
//! call its own. Keeping it here makes those two siblings instead of a cycle.
//! Pure data plus pure rules: no keyring, no files, no transport.
use serde::{Deserialize, Serialize};

pub(crate) const ALLOWED_TOOLS: [&str; 8] = [
    "claude_code",
    "claude_desktop",
    "codex_desktop",
    "pi",
    "opencode",
    "dsh_web",
    "workbuddy",
    "dsh_desktop",
];

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ToolCredential {
    pub(crate) api_key: String,
    pub(crate) origin: String,
    pub(crate) model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) local_gateway_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) codex_transport: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) claude_transport: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) models: Vec<ToolModelRoute>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ToolModelRoute {
    pub(crate) model_id: String,
    pub(crate) billing_group: String,
    pub(crate) api_key: String,
    pub(crate) origin: String,
    pub(crate) claude_transport: Option<String>,
    pub(crate) codex_transport: Option<String>,
}

impl std::fmt::Debug for ToolCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolCredential")
            .field("model_count", &self.model_ids().len())
            .field("credentials", &"[redacted]")
            .finish()
    }
}

impl ToolCredential {
    pub(crate) fn has_model_set(&self) -> bool {
        !self.models.is_empty()
    }
    pub(crate) fn model_ids(&self) -> Vec<String> {
        if self.models.is_empty() {
            vec![self.model_id.clone()]
        } else {
            self.models.iter().map(|r| r.model_id.clone()).collect()
        }
    }
    pub(crate) fn resolve_model(&self, requested: &str) -> Result<Self, CredentialFailure> {
        if self.models.is_empty() {
            return (requested == self.model_id)
                .then(|| self.clone())
                .ok_or(CredentialFailure::Invalid);
        }
        let r = self
            .models
            .iter()
            .find(|r| r.model_id == requested)
            .ok_or(CredentialFailure::Invalid)?;
        Ok(Self {
            api_key: r.api_key.clone(),
            origin: r.origin.clone(),
            model_id: r.model_id.clone(),
            local_gateway_token: self.local_gateway_token.clone(),
            codex_transport: r.codex_transport.clone(),
            claude_transport: r.claude_transport.clone(),
            models: Vec::new(),
        })
    }
    /// The default credential a tool presents to its configured endpoint.
    /// Claude Desktop uses a dedicated local capability token; other tools use
    /// their scoped relay key. Claude Code's new gateway helper is separate so
    /// old direct receipts retain this contract.
    pub(crate) fn client_token(&self, tool: &str) -> &str {
        if tool == "claude_desktop" {
            self.local_gateway_token.as_deref().unwrap_or("")
        } else {
            &self.api_key
        }
    }

    /// The scoped per-tool key issued by the relay.
    pub(crate) fn upstream_key(&self) -> &str {
        &self.api_key
    }
}

#[derive(Debug)]
pub(crate) enum CredentialFailure {
    Unavailable,
    Missing,
    Invalid,
}

/// Only a tool with a keychain slot may hold a credential.
pub(crate) fn allowed_tool(tool_id: &str) -> bool {
    ALLOWED_TOOLS.contains(&tool_id)
}
