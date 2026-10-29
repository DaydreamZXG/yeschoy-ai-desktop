//! Help-desk snapshot for the in-app health assistant.
//!
//! The user is asked to paste this into a message to the developer, so it
//! has to answer "why didn't it work on this machine" without ever carrying
//! anything the user would not want to share. Two rules follow from that:
//!
//! - Read-only. Nothing here writes, fixes, launches or connects to a station.
//! - Only stable categories leave the process: platform, version, stage
//!   markers, reason codes and counts. The on-disk log is reduced to its last
//!   `TAIL_LINES` lines per category, truncated, and scrubbed of anything that
//!   looks like a path, a URL, an IP or a credential. Upstream code may still
//!   log a surprising value one day; the scrubber is the backstop, not a
//!   substitute for "do not log secrets" (see AGENTS.md invariant 7).
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Enough timeline for a support conversation, small enough to paste in chat.
const TAIL_LINES: usize = 120;
const MAX_LINE_CHARS: usize = 240;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HealthSnapshot {
    pub(crate) platform: &'static str,
    pub(crate) arch: &'static str,
    pub(crate) app_version: &'static str,
    /// `bundled-custom-protocol` for installed builds, `dev-server` for `pnpm dev`.
    /// A support thread that assumes a release build and is looking at a dev
    /// process wastes both ends' time.
    pub(crate) frontend_mode: &'static str,
    /// `community`, `official` or `partner`, decided at compile time.
    pub(crate) distribution: &'static str,
    pub(crate) log_available: bool,
    /// Scrubbed lines, oldest first.
    pub(crate) log_tail: Vec<String>,
    /// Normalized `key=value` pairs from the most recent start banner
    /// (`element`/`detail`), for a compact "what environment" header.
    pub(crate) markers: Vec<HealthMarker>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HealthMarker {
    pub(crate) element: String,
    pub(crate) detail: String,
}

fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

fn arch() -> &'static str {
    std::env::consts::ARCH
}

fn frontend_mode() -> &'static str {
    if cfg!(feature = "custom-protocol") {
        "bundled-custom-protocol"
    } else {
        "dev-server"
    }
}

fn distribution() -> &'static str {
    option_env!("YESCHOY_DISTRIBUTION").unwrap_or("community")
}

/// Keep the shape of a log line, drop its payload.
///
/// Interactive terminals are allowed to differ in what could be in a line. A
/// prompt body or a help-center link would otherwise land in a developer's
/// inbox. Redaction runs before truncation so a cut-off secret cannot survive
/// by losing the tail that made it recognisable.
fn scrub(line: &str) -> String {
    let mut out = String::with_capacity(line.len().min(MAX_LINE_CHARS));
    for token in line.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&scrub_token(token));
        if out.chars().count() >= MAX_LINE_CHARS {
            break;
        }
    }
    out.chars().take(MAX_LINE_CHARS).collect()
}

fn scrub_token(token: &str) -> String {
    let (key, value) = match token.split_once('=') {
        Some((key, value)) => (key, Some(value)),
        None => (token, None),
    };
    // Key names are part of the log vocabulary and safe on their own; only
    // the value is judged.
    let Some(value) = value else {
        return redact_bare(token).to_owned();
    };
    let redacted = if looks_sensitive(key) {
        "[redacted]".to_owned()
    } else {
        scrub_value(value)
    };
    format!("{key}={redacted}")
}

fn looks_sensitive(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "key",
        "token",
        "secret",
        "password",
        "authorization",
        "cookie",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

fn scrub_value(value: &str) -> String {
    let unquoted = value.trim_matches(|c| c == '"' || c == '\'');
    // URL / path / IP / raw credential shapes all become the same placeholder:
    // the category of the thing is enough to debug a stage, the thing is not.
    if unquoted.contains("://")
        || unquoted.contains('/')
        || unquoted.contains('\\')
        || looks_like_ip(unquoted)
        || looks_like_secret(unquoted)
    {
        return "[redacted]".to_owned();
    }
    redact_bare(unquoted).to_owned()
}

fn redact_bare(token: &str) -> &str {
    if token.contains("://") || looks_like_ip(token) || looks_like_secret(token) {
        "[redacted]"
    } else {
        token
    }
}

fn looks_like_ip(token: &str) -> bool {
    let candidate = token.trim_matches(|c| c == ':' || c == ',' || c == '(' || c == ')');
    let mut parts = 0;
    let mut all_numeric = true;
    for segment in candidate.split('.') {
        parts += 1;
        if segment.is_empty() || segment.parse::<u16>().is_err() {
            all_numeric = false;
            break;
        }
    }
    all_numeric && parts == 4
}

fn looks_like_secret(token: &str) -> bool {
    // `sk-…`, long opaque blobs (tokens, hashes) and known bearer prefixes.
    if token.starts_with("sk-") || token.starts_with("Bearer") || token.starts_with("bearer") {
        return true;
    }
    token.len() >= 32
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Log location. Mirrors `logging::log_path`, which is private to its module
/// and intentionally not resolvable from the renderer.
fn log_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join("Library")
                .join("Logs")
                .join("野菜API")
                .join("yeschoy.log"),
        )
    }
    #[cfg(target_os = "windows")]
    {
        let local = std::env::var_os("LOCALAPPDATA")?;
        Some(
            PathBuf::from(local)
                .join("野菜API")
                .join("logs")
                .join("yeschoy.log"),
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join(".local")
                .join("state")
                .join("野菜API")
                .join("yeschoy.log"),
        )
    }
}

/// Last `TAIL_LINES` scrubbed lines of a log file. `None` when the file is
/// missing (logging disabled or never started) — the snapshot stays useful.
fn read_tail(path: &Path) -> Option<Vec<String>> {
    let file = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut tail = std::collections::VecDeque::with_capacity(TAIL_LINES);
    for line in reader.lines() {
        let Ok(line) = line else { continue };
        if tail.len() == TAIL_LINES {
            tail.pop_front();
        }
        tail.push_back(scrub(&line));
    }
    Some(tail.into_iter().collect())
}

/// Stable "what environment" header, taken from the newest line per module so
/// the assistant can name the stage without the user reading a log.
fn markers(lines: &[String]) -> Vec<HealthMarker> {
    let mut latest: Vec<(String, String)> = Vec::new();
    for line in lines {
        // Lines look like `<ts> <LEVEL> <target> <message…>`.
        let Some(message) = line.splitn(4, ' ').nth(3) else {
            continue;
        };
        let Some(element) = message.split_whitespace().next() else {
            continue;
        };
        if !element
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            continue;
        }
        let detail = message[element.len()..].trim().to_owned();
        match latest.iter_mut().find(|(name, _)| name == element) {
            Some(tuple) => tuple.1 = detail,
            None => latest.push((element.to_owned(), detail)),
        }
    }
    latest
        .into_iter()
        .take(24)
        .map(|(element, detail)| HealthMarker { element, detail })
        .collect()
}

/// Same `{ request: { requestId } }` envelope as the other IPC commands so the
/// renderer has one request shape to reason about.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HealthSnapshotRequest {
    #[allow(dead_code)]
    pub(crate) request_id: String,
}

#[tauri::command]
pub fn health_snapshot_read_only(request: HealthSnapshotRequest) -> HealthSnapshot {
    let _ = request.request_id;
    let tail = log_path().as_deref().and_then(read_tail);
    let lines = tail.clone().unwrap_or_default();
    let snapshot = HealthSnapshot {
        platform: platform(),
        arch: arch(),
        app_version: env!("CARGO_PKG_VERSION"),
        frontend_mode: frontend_mode(),
        distribution: distribution(),
        log_available: tail.is_some(),
        markers: markers(&lines),
        log_tail: lines,
    };
    // Counts only: never log what the snapshot contained.
    log::info!(
        "health_snapshot stage=read log_available={} lines={} markers={}",
        snapshot.log_available,
        snapshot.log_tail.len(),
        snapshot.markers.len()
    );
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubs_urls_paths_ips_and_credentials() {
        let line = "12.3 INFO mod stage=bridge base_url=https://api.yeschoy.com/v1 \
                    path=/Users/someone/.claude/settings.json peer=203.0.113.9 \
                    token=sk-live-abcdef0123456789 blob=Q2hhbmdlTWUxMjM0NTY3ODkwYWJjZGVmZ2hpamts key=plain";
        let scrubbed = scrub(line);
        for leak in [
            "api.yeschoy.com",
            "/Users/",
            "settings.json",
            "203.0.113.9",
            "sk-live",
            "Q2hhbmdl",
            "plain",
        ] {
            assert!(!scrubbed.contains(leak), "{leak} leaked in {scrubbed}");
        }
        // The vocabulary a developer needs is intact.
        for kept in ["stage=bridge", "base_url=", "token=", "key=", "[redacted]"] {
            assert!(scrubbed.contains(kept), "{kept} missing in {scrubbed}");
        }
    }

    #[test]
    fn keeps_ordinary_reason_codes_readable() {
        let line =
            "1793171656.160 INFO yeschoy_desktop_lib::tool_discovery tool_discovery executable=claude path_directories=4 candidates=2";
        assert_eq!(scrub(line), line);
    }

    #[test]
    fn truncates_long_lines_after_redaction() {
        let line = format!("stage=x note={}", "a".repeat(1000));
        let scrubbed = scrub(&line);
        assert!(scrubbed.chars().count() <= MAX_LINE_CHARS);
        assert!(scrubbed.starts_with("stage=x note="));
    }

    #[test]
    fn tail_keeps_only_the_newest_lines_and_survives_a_missing_file() {
        let dir = std::env::temp_dir().join(format!("yeschoy-health-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("yeschoy.log");
        let body: String = (0..(TAIL_LINES + 20))
            .map(|i| format!("t INFO mod stage=line_{i}\n"))
            .collect();
        std::fs::write(&path, body).unwrap();

        let tail = read_tail(&path).unwrap();
        assert_eq!(tail.len(), TAIL_LINES);
        assert_eq!(tail.last().unwrap(), "t INFO mod stage=line_139");

        assert!(read_tail(&dir.join("missing.log")).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn markers_report_the_newest_line_per_module() {
        let lines = vec![
            "1 INFO a::b desktop_shell stage=start".to_owned(),
            "2 INFO c::d tool_discovery executable=claude".to_owned(),
            "3 INFO a::b desktop_shell stage=ready".to_owned(),
        ];
        assert_eq!(
            markers(&lines),
            vec![
                HealthMarker {
                    element: "desktop_shell".into(),
                    detail: "stage=ready".into(),
                },
                HealthMarker {
                    element: "tool_discovery".into(),
                    detail: "executable=claude".into(),
                },
            ]
        );
    }
}
