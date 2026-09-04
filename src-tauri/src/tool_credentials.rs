use std::io::{self, Read, Write};

use keyring::v1::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};

const SERVICE: &str = "com.yeschoy.desktop.tool-credential.v2";
const ALLOWED_TOOLS: [&str; 7] = [
    "claude_code",
    "claude_desktop",
    "codex_desktop",
    "pi",
    "dsh_web",
    "hermes",
    "openclaw",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
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
}

#[derive(Debug)]
pub(crate) enum CredentialFailure {
    Unavailable,
    Missing,
    Invalid,
}

fn allowed_tool(tool_id: &str) -> bool {
    ALLOWED_TOOLS.contains(&tool_id)
}

fn entry(tool_id: &str) -> Result<Entry, CredentialFailure> {
    if !allowed_tool(tool_id) {
        return Err(CredentialFailure::Invalid);
    }
    Entry::new(SERVICE, tool_id).map_err(|_| CredentialFailure::Unavailable)
}

fn record_is_valid(tool_id: &str, record: &ToolCredential) -> bool {
    let key = record.api_key.as_str();
    let model = record.model_id.as_str();
    let local = record.local_gateway_token.as_deref().unwrap_or("");
    (16..=256).contains(&key.len())
        && !key
            .chars()
            .any(|value| value.is_control() || value.is_whitespace())
        && matches!(
            record.origin.as_str(),
            "https://yeschoy.com" | "https://api.yeschoy.com"
        )
        && !model.is_empty()
        && model.chars().count() <= 200
        && !model.chars().any(char::is_control)
        && (record.codex_transport.is_none()
            || (tool_id == "codex_desktop"
                && matches!(
                    record.codex_transport.as_deref(),
                    Some("direct_responses" | "chat_bridge")
                )))
        && (record.claude_transport.is_none()
            || (matches!(tool_id, "claude_code" | "claude_desktop")
                && matches!(
                    record.claude_transport.as_deref(),
                    Some("direct_anthropic" | "chat_bridge")
                )))
        && (local.is_empty()
            || ((32..=256).contains(&local.len())
                && !local
                    .chars()
                    .any(|value| value.is_control() || value.is_whitespace())))
        && (tool_id != "claude_desktop" || !local.is_empty())
        && (record.claude_transport.as_deref() != Some("chat_bridge") || !local.is_empty())
}

pub(crate) fn load(tool_id: &str) -> Result<ToolCredential, CredentialFailure> {
    let payload = match entry(tool_id)?.get_password() {
        Ok(value) => value,
        Err(KeyringError::NoEntry) => return Err(CredentialFailure::Missing),
        Err(_) => return Err(CredentialFailure::Unavailable),
    };
    let record: ToolCredential =
        serde_json::from_str(&payload).map_err(|_| CredentialFailure::Invalid)?;
    record_is_valid(tool_id, &record)
        .then_some(record)
        .ok_or(CredentialFailure::Invalid)
}

pub(crate) fn snapshot(tool_id: &str) -> Result<Option<String>, CredentialFailure> {
    match entry(tool_id)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(_) => Err(CredentialFailure::Unavailable),
    }
}

pub(crate) fn store(tool_id: &str, record: &ToolCredential) -> Result<(), CredentialFailure> {
    if !record_is_valid(tool_id, record) {
        return Err(CredentialFailure::Invalid);
    }
    let payload = serde_json::to_string(record).map_err(|_| CredentialFailure::Invalid)?;
    entry(tool_id)?
        .set_password(&payload)
        .map_err(|_| CredentialFailure::Unavailable)
}

pub(crate) fn restore(tool_id: &str, before: Option<&str>) -> Result<(), CredentialFailure> {
    let entry = entry(tool_id)?;
    match before {
        Some(value) => entry
            .set_password(value)
            .map_err(|_| CredentialFailure::Unavailable),
        None => match entry.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(_) => Err(CredentialFailure::Unavailable),
        },
    }
}

pub(crate) fn executable_path() -> Result<String, CredentialFailure> {
    let executable = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|_| CredentialFailure::Unavailable)?;
    let rendered = executable
        .to_str()
        .filter(|value| !value.is_empty())
        .ok_or(CredentialFailure::Unavailable)?;
    Ok(rendered.to_owned())
}

#[cfg(not(target_os = "windows"))]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "windows")]
fn shell_quote(value: &str) -> String {
    // The helper path is compiled into a command string consumed by cmd.exe in
    // the Windows builds of Claude Code and Pi. Reject shell metacharacters
    // instead of trying to invent another parsing dialect.
    if value.chars().any(|character| {
        matches!(
            character,
            '"' | '&' | '|' | '<' | '>' | '^' | '%' | '!' | '\r' | '\n'
        )
    }) {
        return String::new();
    }
    format!("\"{value}\"")
}

pub(crate) fn shell_helper_command(tool_id: &str) -> Result<String, CredentialFailure> {
    if !allowed_tool(tool_id) {
        return Err(CredentialFailure::Invalid);
    }
    let executable = shell_quote(&executable_path()?);
    if executable.is_empty() {
        return Err(CredentialFailure::Unavailable);
    }
    Ok(format!("{executable} credential-helper {tool_id}"))
}

fn bare_helper(tool_id: &str) -> i32 {
    match load(tool_id) {
        Ok(record) => {
            let secret = if tool_id == "claude_code"
                && record.claude_transport.as_deref() == Some("chat_bridge")
            {
                record.local_gateway_token.as_deref().unwrap_or("")
            } else {
                record.api_key.as_str()
            };
            let mut output = io::stdout().lock();
            if output.write_all(secret.as_bytes()).is_ok()
                && output.write_all(b"\n").is_ok()
                && output.flush().is_ok()
            {
                0
            } else {
                71
            }
        }
        Err(CredentialFailure::Missing) => 69,
        Err(CredentialFailure::Unavailable) => 70,
        Err(CredentialFailure::Invalid) => 65,
    }
}

fn openclaw_helper(tool_id: &str) -> i32 {
    const ID: &str = "providers/yeschoy/apiKey";
    let mut request = Vec::new();
    if io::stdin()
        .lock()
        .take(16 * 1024 + 1)
        .read_to_end(&mut request)
        .is_err()
        || request.len() > 16 * 1024
    {
        return 65;
    }
    let valid_request = serde_json::from_slice::<serde_json::Value>(&request)
        .ok()
        .and_then(|value| {
            let object = value.as_object()?;
            let ids = object.get("ids")?.as_array()?;
            (object.get("protocolVersion")?.as_u64()? == 1
                && object.get("provider")?.as_str()? == "yeschoy-keychain"
                && ids.len() == 1
                && ids[0].as_str()? == ID)
                .then_some(())
        })
        .is_some();
    if !valid_request {
        return 65;
    }
    let record = match load(tool_id) {
        Ok(value) => value,
        Err(CredentialFailure::Missing) => return 69,
        Err(CredentialFailure::Unavailable) => return 70,
        Err(CredentialFailure::Invalid) => return 65,
    };
    let response = serde_json::json!({
        "protocolVersion": 1,
        "values": { ID: record.api_key }
    });
    let mut output = io::stdout().lock();
    if serde_json::to_writer(&mut output, &response).is_ok()
        && output.write_all(b"\n").is_ok()
        && output.flush().is_ok()
    {
        0
    } else {
        71
    }
}

/// Intercepts signed helper modes before Tauri is initialized. Bare-token
/// consumers use `credential-helper`; OpenClaw uses its bounded protocol-v1
/// exec SecretRef exchange.
pub fn credential_helper_exit_code() -> Option<i32> {
    let arguments = std::env::args().collect::<Vec<_>>();
    let code = match arguments.get(1).map(String::as_str) {
        Some("credential-helper") => match arguments.as_slice() {
            [_, _, tool_id] if allowed_tool(tool_id) => bare_helper(tool_id),
            _ => 64,
        },
        Some("credential-helper-openclaw") => match arguments.as_slice() {
            [_, _, tool_id] if tool_id == "openclaw" => openclaw_helper("openclaw"),
            _ => 64,
        },
        _ => return None,
    };
    Some(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_command_contains_no_secret_and_quotes_the_executable() {
        let command = shell_helper_command("pi").expect("helper command");
        assert!(command.contains("credential-helper pi"));
        assert!(!command.contains("sk-"));
        assert!(command.starts_with(['\'', '"']));
    }

    #[test]
    fn rejects_unknown_helper_targets() {
        assert!(matches!(
            shell_helper_command("opencode"),
            Err(CredentialFailure::Invalid)
        ));
    }

    #[test]
    fn openclaw_is_allowlisted_but_uses_a_separate_protocol_mode() {
        assert!(allowed_tool("openclaw"));
        assert!(shell_helper_command("openclaw")
            .expect("command")
            .contains("credential-helper openclaw"));
    }

    #[test]
    fn transport_metadata_is_codex_only_and_backward_compatible() {
        let mut record = ToolCredential {
            api_key: "synthetic-test-key-0001".into(),
            origin: "https://yeschoy.com".into(),
            model_id: "gpt-5.5".into(),
            local_gateway_token: None,
            codex_transport: None,
            claude_transport: None,
        };
        assert!(record_is_valid("codex_desktop", &record));
        record.codex_transport = Some("chat_bridge".into());
        assert!(record_is_valid("codex_desktop", &record));
        assert!(!record_is_valid("pi", &record));
        record.codex_transport = Some("caller_route".into());
        assert!(!record_is_valid("codex_desktop", &record));
    }

    #[test]
    fn claude_transport_requires_a_local_token_only_for_bridged_code() {
        let mut record = ToolCredential {
            api_key: "synthetic-test-key-0001".into(),
            origin: "https://yeschoy.com".into(),
            model_id: "claude-opus-5".into(),
            local_gateway_token: None,
            codex_transport: None,
            claude_transport: Some("direct_anthropic".into()),
        };
        assert!(record_is_valid("claude_code", &record));
        record.claude_transport = Some("chat_bridge".into());
        assert!(!record_is_valid("claude_code", &record));
        record.local_gateway_token = Some(format!("ycg-{}", "a".repeat(64)));
        assert!(record_is_valid("claude_code", &record));
        assert!(record_is_valid("claude_desktop", &record));
        assert!(!record_is_valid("pi", &record));
    }
}
