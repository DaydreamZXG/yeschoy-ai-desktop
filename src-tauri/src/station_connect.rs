//! Station deep-link connect protocol (`yeschoy://connect`) — parse, validation,
//! and OS/runtime delivery. Code exchange and UI wiring land in later tasks.
//! This module is intentionally separate from the compile-time authorization-page
//! allowlist in `account_v2` / `oauth_pkce`.

use reqwest::Url;
use tauri::{AppHandle, Runtime};
use tauri_plugin_deep_link::DeepLinkExt;

pub(crate) const CONNECT_SCHEME: &str = "yeschoy";
pub(crate) const CONNECT_HOST: &str = "connect";

/// Minimum length for a one-time connect `code` (bytes, UTF-8).
pub(crate) const CONNECT_CODE_MIN_LEN: usize = 32;
/// Maximum length for a one-time connect `code` (bytes, UTF-8).
pub(crate) const CONNECT_CODE_MAX_LEN: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StationConnectParseError {
    InvalidDeepLinkUrl,
    WrongScheme,
    WrongHost,
    UnexpectedQueryParameter,
    DuplicateQueryParameter,
    MissingOrigin,
    MissingCode,
    InvalidOrigin,
    InvalidCode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StationConnectRequest {
    /// Canonical HTTPS origin (`https://host`, lowercase host, no path/query).
    pub(crate) origin: String,
    pub(crate) code: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum StationConnectHandleOutcome {
    /// Parsed and validated; exchange is deferred to a later task.
    Accepted(StationConnectRequest),
    /// Not a `yeschoy://` URL (other argv or schemes are ignored).
    IgnoredNotConnectScheme,
    /// Looked like station connect but failed validation.
    Rejected(StationConnectParseError),
}

/// Single entry point for cold-start argv, hot-start deep-link events, and tests.
pub(crate) fn handle_connect_url(raw: &str) -> StationConnectHandleOutcome {
    if !raw.starts_with("yeschoy://") {
        return StationConnectHandleOutcome::IgnoredNotConnectScheme;
    }
    match parse_station_connect_url(raw) {
        Ok(request) => {
            log::info!(
                "station_connect stage=validated origin={}",
                request.origin
            );
            StationConnectHandleOutcome::Accepted(request)
        }
        Err(error) => {
            log::warn!("station_connect stage=parse_rejected error={error:?}");
            StationConnectHandleOutcome::Rejected(error)
        }
    }
}

fn deliver_connect_urls<S: AsRef<str>>(urls: &[S]) {
    for url in urls {
        handle_connect_url(url.as_ref());
    }
}

/// Wire `tauri-plugin-deep-link` and drain any URLs already captured at startup.
pub(crate) fn wire_station_connect_deep_link<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<(), tauri_plugin_deep_link::Error> {
    app.deep_link().on_open_url(move |event| {
        let urls = event
            .urls()
            .into_iter()
            .map(|url| url.to_string())
            .collect::<Vec<_>>();
        deliver_connect_urls(&urls);
    });

    if let Some(urls) = app.deep_link().get_current()? {
        let urls = urls
            .into_iter()
            .map(|url| url.to_string())
            .collect::<Vec<_>>();
        deliver_connect_urls(&urls);
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    if let Err(error) = app.deep_link().register_all() {
        log::warn!("station_connect stage=register_all_failed error={error}");
    }

    Ok(())
}

/// Returns the canonical HTTPS origin when `raw` is acceptable for connect exchange.
///
/// This is **not** the compile-time authorization-page allowlist; third-party
/// HTTPS origins are allowed when they pass structural checks.
pub(crate) fn connect_origin_is_acceptable(raw: &str) -> Option<String> {
    let Ok(url) = Url::parse(raw) else {
        return None;
    };
    if !origin_url_structure_is_valid(&url) {
        return None;
    }
    let host = url.host_str()?;
    if host_is_literal_ip(host) {
        return None;
    }
    let canonical = canonical_https_origin_from_host(host);
    if raw != canonical && raw != format!("{canonical}/") {
        return None;
    }
    Some(canonical)
}

pub(crate) fn connect_code_is_valid(value: &str) -> bool {
    (CONNECT_CODE_MIN_LEN..=CONNECT_CODE_MAX_LEN).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub(crate) fn parse_station_connect_url(
    raw: &str,
) -> Result<StationConnectRequest, StationConnectParseError> {
    let url = Url::parse(raw).map_err(|_| StationConnectParseError::InvalidDeepLinkUrl)?;
    if url.scheme() != CONNECT_SCHEME {
        return Err(StationConnectParseError::WrongScheme);
    }
    if url.host_str() != Some(CONNECT_HOST) {
        return Err(StationConnectParseError::WrongHost);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(StationConnectParseError::InvalidDeepLinkUrl);
    }
    if url.port().is_some() {
        return Err(StationConnectParseError::InvalidDeepLinkUrl);
    }
    let path = url.path();
    if !path.is_empty() && path != "/" {
        return Err(StationConnectParseError::InvalidDeepLinkUrl);
    }
    if url.fragment().is_some() {
        return Err(StationConnectParseError::InvalidDeepLinkUrl);
    }

    let (origin_raw, code) = parse_connect_query(&url)?;
    let origin =
        connect_origin_is_acceptable(&origin_raw).ok_or(StationConnectParseError::InvalidOrigin)?;
    if !connect_code_is_valid(&code) {
        return Err(StationConnectParseError::InvalidCode);
    }
    Ok(StationConnectRequest { origin, code })
}

fn origin_url_structure_is_valid(url: &Url) -> bool {
    url.scheme() == "https"
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(url.path(), "/" | "")
        && url.query().is_none()
        && url.fragment().is_none()
        && url.host_str().is_some_and(|host| !host.is_empty())
}

fn canonical_https_origin_from_host(host: &str) -> String {
    format!("https://{}", host.to_ascii_lowercase())
}

fn host_is_literal_ip(host: &str) -> bool {
    let bare = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    bare.parse::<std::net::Ipv4Addr>().is_ok() || bare.parse::<std::net::Ipv6Addr>().is_ok()
}

fn parse_connect_query(url: &Url) -> Result<(String, String), StationConnectParseError> {
    if url.query().is_none() {
        return Err(StationConnectParseError::MissingOrigin);
    }
    let mut origin: Option<String> = None;
    let mut code: Option<String> = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "origin" => {
                if origin.is_some() {
                    return Err(StationConnectParseError::DuplicateQueryParameter);
                }
                origin = Some(value.into_owned());
            }
            "code" => {
                if code.is_some() {
                    return Err(StationConnectParseError::DuplicateQueryParameter);
                }
                code = Some(value.into_owned());
            }
            _ => return Err(StationConnectParseError::UnexpectedQueryParameter),
        }
    }
    let origin = origin.ok_or(StationConnectParseError::MissingOrigin)?;
    let code = code.ok_or(StationConnectParseError::MissingCode)?;
    Ok((origin, code))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_CODE: &str = "abcdefghijklmnopqrstuvwxyz012345";

    #[test]
    fn connect_origin_accepts_third_party_https_canonical() {
        assert_eq!(
            connect_origin_is_acceptable("https://example.com"),
            Some("https://example.com".to_owned())
        );
        assert_eq!(
            connect_origin_is_acceptable("https://ai.yeschoy.io"),
            Some("https://ai.yeschoy.io".to_owned())
        );
    }

    #[test]
    fn connect_origin_rejects_non_canonical_and_malicious_shapes() {
        for rejected in [
            "http://example.com",
            "https://example.com/path",
            "https://example.com?x=1",
            "https://example.com#frag",
            "https://user:pass@example.com",
            "https://example.com:443",
            "https://127.0.0.1",
            "https://[::1]",
            "https://EXAMPLE.COM",
            "not-a-url",
        ] {
            assert!(
                connect_origin_is_acceptable(rejected).is_none(),
                "expected reject: {rejected}"
            );
        }
    }

    #[test]
    fn connect_origin_is_independent_of_authorization_page_allowlist() {
        assert!(connect_origin_is_acceptable("https://third-party.example").is_some());
        // Lookalike hostnames are structurally valid; device-code allowlist rejects
        // these, but connect exchange binds code to the declared origin instead.
        assert!(connect_origin_is_acceptable("https://yeschoy.com.evil.invalid").is_some());
    }

    #[test]
    fn connect_code_enforces_url_safe_length_bounds() {
        assert!(connect_code_is_valid(VALID_CODE));
        assert!(!connect_code_is_valid("short"));
        let oversized = "a".repeat(CONNECT_CODE_MAX_LEN + 1);
        assert!(!connect_code_is_valid(&oversized));
        assert!(!connect_code_is_valid(&format!("{VALID_CODE}!")));
    }

    #[test]
    fn handle_connect_url_accepts_valid_and_ignores_other_schemes() {
        let raw = format!("yeschoy://connect?origin=https%3A%2F%2Fexample.com&code={VALID_CODE}");
        let outcome = handle_connect_url(&raw);
        assert!(matches!(
            outcome,
            StationConnectHandleOutcome::Accepted(StationConnectRequest {
                origin,
                code
            }) if origin == "https://example.com" && code == VALID_CODE
        ));
        assert_eq!(
            handle_connect_url("https://yeschoy.com"),
            StationConnectHandleOutcome::IgnoredNotConnectScheme
        );
        assert!(matches!(
            handle_connect_url("yeschoy://connect?code=only"),
            StationConnectHandleOutcome::Rejected(StationConnectParseError::MissingOrigin)
        ));
    }

    #[test]
    fn parse_accepts_well_formed_deep_link() {
        let raw = format!("yeschoy://connect?origin=https%3A%2F%2Fexample.com&code={VALID_CODE}");
        let parsed = parse_station_connect_url(&raw).unwrap();
        assert_eq!(parsed.origin, "https://example.com");
        assert_eq!(parsed.code, VALID_CODE);
    }

    #[test]
    fn parse_rejects_wrong_scheme_host_and_query_issues() {
        let base = format!("yeschoy://connect?origin=https%3A%2F%2Fexample.com&code={VALID_CODE}");
        assert_eq!(
            parse_station_connect_url(base.replace("yeschoy://", "http://").as_str()),
            Err(StationConnectParseError::WrongScheme)
        );
        assert_eq!(
            parse_station_connect_url(base.replace("/connect?", "/login?").as_str()),
            Err(StationConnectParseError::WrongHost)
        );
        assert_eq!(
            parse_station_connect_url(
                format!(
                    "yeschoy://connect?origin=https%3A%2F%2Fexample.com&code={VALID_CODE}&extra=1"
                )
                .as_str()
            ),
            Err(StationConnectParseError::UnexpectedQueryParameter)
        );
        assert_eq!(
            parse_station_connect_url("yeschoy://connect?code=only").unwrap_err(),
            StationConnectParseError::MissingOrigin
        );
        assert_eq!(
            parse_station_connect_url("yeschoy://connect?origin=https%3A%2F%2Fexample.com")
                .unwrap_err(),
            StationConnectParseError::MissingCode
        );
        assert_eq!(
            parse_station_connect_url(
                format!(
                    "yeschoy://connect?origin=https%3A%2F%2Fexample.com&origin=https%3A%2F%2Fexample.com&code={VALID_CODE}"
                )
                .as_str()
            ),
            Err(StationConnectParseError::DuplicateQueryParameter)
        );
    }

    #[test]
    fn parse_rejects_http_origin_path_and_invalid_code() {
        let http_origin =
            format!("yeschoy://connect?origin=http%3A%2F%2Fexample.com&code={VALID_CODE}");
        assert_eq!(
            parse_station_connect_url(&http_origin),
            Err(StationConnectParseError::InvalidOrigin)
        );
        let path_origin = format!(
            "yeschoy://connect?origin=https%3A%2F%2Fexample.com%2Fwallet&code={VALID_CODE}"
        );
        assert_eq!(
            parse_station_connect_url(&path_origin),
            Err(StationConnectParseError::InvalidOrigin)
        );
        let bad_code = format!(
            "yeschoy://connect?origin=https%3A%2F%2Fexample.com&code={}",
            "a".repeat(CONNECT_CODE_MAX_LEN + 1)
        );
        assert_eq!(
            parse_station_connect_url(&bad_code),
            Err(StationConnectParseError::InvalidCode)
        );
    }
}
