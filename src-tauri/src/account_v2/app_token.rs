//! The device credential a station issues to this client.
//!
//! A station may price some plans for requests that come through a desktop
//! client. It tells such requests apart by a credential it issued to this
//! device, sent in a header the station names in its bootstrap. The credential
//! is not a sign-in: it grants nothing by itself, and only the local
//! forwarders attach it, only to the station that issued it.
//!
//! Each station has its own credential. Tools stay connected to a station
//! after the user signs in to another one, and their requests still need the
//! credential of the station they go to.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use reqwest::header::{HeaderName, HeaderValue};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{send_json, NeutralPaths, KEYRING_SERVICE};

const KEYRING_ACCOUNT_PREFIX: &str = "desktop-app-token:";
const RETRY_AFTER: Duration = Duration::from_secs(300);
const TOKEN_MAX_LEN: usize = 512;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Issued {
    origin: String,
    header: String,
    token: String,
    device_id: String,
}

struct State {
    issued: Vec<Issued>,
    /// Stations asked for a credential recently, so a refusal is not retried
    /// on every refresh.
    attempts: Vec<(String, Instant)>,
    /// Stations whose stored credential was already looked up this run.
    looked_up: Vec<String>,
}

static STATE: Mutex<State> = Mutex::new(State {
    issued: Vec::new(),
    attempts: Vec::new(),
    looked_up: Vec::new(),
});

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn keyring_account(origin: &str) -> String {
    format!("{KEYRING_ACCOUNT_PREFIX}{origin}")
}

/// `scheme://host[:port]` of an absolute URL.
fn origin_of(url: &str) -> Option<&str> {
    let after_scheme = url.find("://")? + 3;
    let end = url[after_scheme..]
        .find(['/', '?', '#'])
        .map_or(url.len(), |at| after_scheme + at);
    (end > after_scheme).then(|| &url[..end])
}

#[cfg(not(test))]
fn read_stored(origin: &str) -> Option<Issued> {
    crate::legacy::read_secret(KEYRING_SERVICE, &keyring_account(origin))
        .ok()
        .flatten()
        .and_then(|payload| serde_json::from_str::<Issued>(&payload).ok())
        .filter(|issued| issued.origin == origin)
}

// Tests never touch the OS credential store.
#[cfg(test)]
fn read_stored(_origin: &str) -> Option<Issued> {
    None
}

fn remember(state: &mut State, issued: Issued) {
    state.issued.retain(|known| known.origin != issued.origin);
    state.issued.push(issued);
}

/// The credential for the station `url` is on: the one in memory, or the one
/// stored on an earlier run for a station that is not the signed-in one.
fn credential_for(url: &str) -> Option<Issued> {
    if let Some(known) = state()
        .issued
        .iter()
        .find(|issued| belongs_to(url, &issued.origin))
    {
        return Some(known.clone());
    }
    let origin = origin_of(url)?;
    {
        let mut state = state();
        if state.looked_up.iter().any(|seen| seen == origin) {
            return None;
        }
        state.looked_up.push(origin.to_owned());
    }
    let stored = read_stored(origin)?;
    remember(&mut state(), stored.clone());
    Some(stored)
}

fn belongs_to(url: &str, origin: &str) -> bool {
    url.strip_prefix(origin)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

fn header_of(issued: &Issued, url: &str) -> Option<(HeaderName, HeaderValue)> {
    if !belongs_to(url, &issued.origin) {
        return None;
    }
    let mut value = HeaderValue::from_str(&issued.token).ok()?;
    value.set_sensitive(true);
    Some((
        HeaderName::from_bytes(issued.header.as_bytes()).ok()?,
        value,
    ))
}

fn with_credential(
    request: reqwest::RequestBuilder,
    issued: &Issued,
    url: &str,
) -> reqwest::RequestBuilder {
    match header_of(issued, url) {
        Some((name, value)) => {
            let mut headers = reqwest::header::HeaderMap::with_capacity(1);
            headers.insert(name, value);
            request.headers(headers)
        }
        None => request,
    }
}

/// Adds the device credential to a request the local forwarders send upstream,
/// when `url` is on a station that issued one. Call it after the application's
/// own headers are copied: it replaces whatever the request already carries
/// under that name, so the station never sees two values.
pub(crate) fn attach(request: reqwest::RequestBuilder, url: &str) -> reqwest::RequestBuilder {
    match credential_for(url) {
        Some(issued) => with_credential(request, &issued, url),
        None => request,
    }
}

/// A header only this client may set on an upstream request. Whatever an
/// application sends under such a name is dropped, so the station never sees
/// two values.
pub(crate) fn is_reserved(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with("-app-token")
        || state()
            .issued
            .iter()
            .any(|issued| issued.header.eq_ignore_ascii_case(name))
}

/// The credential in a station's reply, if it is one we can send in a header.
fn issued_token(status: u16, value: &Value) -> Option<String> {
    if !(200..300).contains(&status) || value.get("success").and_then(Value::as_bool) != Some(true)
    {
        return None;
    }
    let token = value.get("data")?.get("appToken")?.as_str()?;
    (!token.is_empty()
        && token.len() <= TOKEN_MAX_LEN
        && token.bytes().all(|byte| byte.is_ascii_graphic()))
    .then(|| token.to_owned())
}

/// Makes sure this device holds the station's credential. Failing is quiet:
/// the station then serves requests at whatever it charges without one.
pub(super) async fn ensure(origin: &str, paths: &NeutralPaths, access_token: &str) {
    if paths.app_tokens.is_empty() || paths.app_token_header.is_empty() {
        return;
    }
    let current =
        |issued: &Issued| issued.origin == origin && issued.header == paths.app_token_header;
    {
        let mut state = state();
        if state.issued.iter().any(current) {
            return;
        }
        // The station may rename its header; the credential itself is unchanged.
        if let Some(known) = state
            .issued
            .iter_mut()
            .find(|issued| issued.origin == origin)
        {
            known.header.clone_from(&paths.app_token_header);
            return;
        }
        if state
            .attempts
            .iter()
            .any(|(at, when)| at == origin && when.elapsed() < RETRY_AFTER)
        {
            return;
        }
        state.attempts.retain(|(at, _)| at != origin);
        state.attempts.push((origin.to_owned(), Instant::now()));
    }
    if let Some(mut issued) = read_stored(origin) {
        issued.header.clone_from(&paths.app_token_header);
        remember(&mut state(), issued);
        return;
    }
    let device_id = uuid::Uuid::new_v4().to_string();
    let body = json!({
        "deviceId": device_id,
        "platform": crate::station_connect_exchange::connect_platform(),
        "clientVersion": env!("CARGO_PKG_VERSION"),
    });
    let url = format!("{origin}{}", paths.app_tokens);
    let token = match send_json(Method::POST, &url, Some(access_token), Some(body)).await {
        Ok(response) => issued_token(response.status.as_u16(), &response.value),
        Err(_) => None,
    };
    let Some(token) = token else {
        log::warn!("account stage=app_token_unavailable");
        return;
    };
    let issued = Issued {
        origin: origin.to_owned(),
        header: paths.app_token_header.clone(),
        token,
        device_id,
    };
    if let Ok(payload) = serde_json::to_string(&issued) {
        if crate::legacy::write_secret(KEYRING_SERVICE, &keyring_account(origin), &payload).is_err()
        {
            // Still usable until the client exits; it is asked for again next time.
            log::warn!("account stage=app_token_not_stored");
        }
    }
    log::info!("account stage=app_token_issued");
    remember(&mut state(), issued);
}

/// Signing out of a station, or signing in to it again, ends its credential:
/// it was issued to the account that was signed in there. Other stations'
/// credentials stay.
pub(super) fn forget(origin: &str) {
    {
        let mut state = state();
        state.issued.retain(|issued| issued.origin != origin);
        state.attempts.retain(|(at, _)| at != origin);
        state.looked_up.retain(|seen| seen != origin);
    }
    let _ = crate::legacy::delete_secret(KEYRING_SERVICE, &keyring_account(origin));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issued() -> Issued {
        Issued {
            origin: "https://station.example".into(),
            header: "X-Desktop-App-Token".into(),
            token: "dat_synthetic".into(),
            device_id: "device".into(),
        }
    }

    #[test]
    fn the_credential_only_goes_to_the_station_that_issued_it() {
        let (name, value) =
            header_of(&issued(), "https://station.example/v1/messages").expect("same station");
        assert_eq!(name.as_str(), "x-desktop-app-token");
        assert_eq!(value.to_str().unwrap(), "dat_synthetic");
        assert!(value.is_sensitive());
        for elsewhere in [
            "https://station.example.evil.test/v1/messages",
            "https://other.example/v1/messages",
            "http://station.example/v1/messages",
            "https://station.example:8443/v1/messages",
        ] {
            assert!(header_of(&issued(), elsewhere).is_none(), "{elsewhere}");
        }
    }

    #[test]
    fn each_station_keeps_its_own_credential() {
        let other = Issued {
            origin: "https://other.example".into(),
            header: "X-Other-Pass".into(),
            token: "dat_other".into(),
            device_id: "device-2".into(),
        };
        let mut state = State {
            issued: Vec::new(),
            attempts: Vec::new(),
            looked_up: Vec::new(),
        };
        remember(&mut state, issued());
        remember(&mut state, other.clone());
        // Issuing again for a station replaces only that station's.
        remember(
            &mut state,
            Issued {
                token: "dat_renewed".into(),
                ..issued()
            },
        );
        let token_for = |url: &str| {
            state
                .issued
                .iter()
                .find_map(|issued| header_of(issued, url))
                .map(|(_, value)| value.to_str().unwrap().to_owned())
        };
        assert_eq!(
            token_for("https://station.example/v1/messages").as_deref(),
            Some("dat_renewed")
        );
        assert_eq!(
            token_for("https://other.example/v1/chat/completions").as_deref(),
            Some("dat_other")
        );
        assert_eq!(token_for("https://third.example/v1/messages"), None);
        assert_eq!(state.issued.len(), 2);
    }

    #[test]
    fn the_credential_replaces_a_value_the_application_sent() {
        let url = "https://station.example/v1/messages";
        let sent = reqwest::Client::new()
            .post(url)
            .header("X-Desktop-App-Token", "from-the-application");
        let request = with_credential(sent, &issued(), url).build().unwrap();
        let values: Vec<_> = request
            .headers()
            .get_all("x-desktop-app-token")
            .iter()
            .collect();
        assert_eq!(values, ["dat_synthetic"]);
    }

    #[test]
    fn the_origin_of_a_url_is_its_scheme_host_and_port() {
        assert_eq!(
            origin_of("https://station.example/v1/messages"),
            Some("https://station.example")
        );
        assert_eq!(
            origin_of("https://station.example:8443?x=1"),
            Some("https://station.example:8443")
        );
        assert_eq!(
            origin_of("https://station.example"),
            Some("https://station.example")
        );
        assert_eq!(origin_of("station.example/v1"), None);
        assert_eq!(origin_of("https:///v1"), None);
    }

    #[test]
    fn an_application_cannot_send_its_own_value_under_the_header() {
        assert!(is_reserved("x-desktop-app-token"));
        assert!(is_reserved("X-Yeschoy-App-Token"));
        assert!(!is_reserved("x-app"));
        assert!(!is_reserved("authorization"));
    }

    #[test]
    fn only_a_successful_reply_with_a_sendable_value_is_a_credential() {
        let ok = json!({"success": true, "data": {"appToken": "dat_abc", "deviceId": "d"}});
        assert_eq!(issued_token(200, &ok).as_deref(), Some("dat_abc"));
        assert!(issued_token(500, &ok).is_none());
        assert!(issued_token(200, &json!({"success": false, "code": "device_invalid"})).is_none());
        assert!(issued_token(200, &json!({"success": true, "data": {}})).is_none());
        let newline = json!({"success": true, "data": {"appToken": "dat\r\nX-Evil: 1"}});
        assert!(issued_token(200, &newline).is_none());
        let long = json!({"success": true, "data": {"appToken": "a".repeat(TOKEN_MAX_LEN + 1)}});
        assert!(issued_token(200, &long).is_none());
    }
}
