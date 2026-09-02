use std::io::{self, Write};

use keyring::v1::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};

const SERVICE: &str = "com.yeschoy.desktop.tool-credential.v2";
const ALLOWED_TOOLS: [&str; 5] = [
    "claude_code",
    "claude_desktop",
    "codex_desktop",
    "pi",
    "dsh_web",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ToolCredential {
    pub(crate) api_key: String,
    pub(crate) origin: String,
    pub(crate) model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) local_gateway_token: Option<String>,
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

fn record_is_valid(record: &ToolCredential) -> bool {
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
        && (local.is_empty()
            || ((32..=256).contains(&local.len())
                && !local
                    .chars()
                    .any(|value| value.is_control() || value.is_whitespace())))
}

pub(crate) fn load(tool_id: &str) -> Result<ToolCredential, CredentialFailure> {
    let payload = match entry(tool_id)?.get_password() {
        Ok(value) => value,
        Err(KeyringError::NoEntry) => return Err(CredentialFailure::Missing),
        Err(_) => return Err(CredentialFailure::Unavailable),
    };
    let record: ToolCredential =
        serde_json::from_str(&payload).map_err(|_| CredentialFailure::Invalid)?;
    record_is_valid(&record)
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
    if !record_is_valid(record) {
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
    let executable = std::env::current_exe().map_err(|_| CredentialFailure::Unavailable)?;
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

/// Intercepts the signed application's credential-helper mode before Tauri is
/// initialized. The key is written only to stdout because all three consumers
/// define stdout as their credential-helper boundary.
pub fn credential_helper_exit_code() -> Option<i32> {
    let arguments = std::env::args().collect::<Vec<_>>();
    if arguments.get(1).map(String::as_str) != Some("credential-helper") {
        return None;
    }
    let code = match arguments.as_slice() {
        [_, _, tool_id] if allowed_tool(tool_id) => match load(tool_id) {
            Ok(record) => {
                let mut output = io::stdout().lock();
                if output.write_all(record.api_key.as_bytes()).is_ok()
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
        },
        _ => 64,
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
}
