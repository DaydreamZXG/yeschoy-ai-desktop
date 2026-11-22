//! The device credential a station issues to this client.
//!
//! A station may price some plans for requests that come through a desktop
//! client. It tells such requests apart by a credential it issued to this
//! device, sent in a header the station names in its bootstrap. The credential
//! is not a sign-in: it grants nothing by itself, and only the local
//! forwarders attach it, only to the station that issued it.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use reqwest::header::{HeaderName, HeaderValue};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{send_json, NeutralPaths, KEYRING_SERVICE};

const KEYRING_ACCOUNT: &str = "desktop-app-token";
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
    issued: Option<Issued>,
    last_attempt: Option<(String, Instant)>,
}

static STATE: Mutex<State> = Mutex::new(State {
    issued: None,
    last_attempt: None,
});

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
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

/// Adds the device credential to a request the local forwarders send upstream,
/// when `url` is on the station that issued it.
pub(crate) fn attach(request: reqwest::RequestBuilder, url: &str) -> reqwest::RequestBuilder {
    match state()
        .issued
        .as_ref()
        .and_then(|issued| header_of(issued, url))
    {
        Some((name, value)) => request.header(name, value),
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
            .as_ref()
            .is_some_and(|issued| issued.header.eq_ignore_ascii_case(name))
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
        if state.issued.as_ref().is_some_and(current) {
            return;
        }
        if state
            .last_attempt
            .as_ref()
            .is_some_and(|(at, when)| at == origin && when.elapsed() < RETRY_AFTER)
        {
            return;
        }
        state.last_attempt = Some((origin.to_owned(), Instant::now()));
    }
    let stored = crate::legacy::read_secret(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .ok()
        .flatten()
        .and_then(|payload| serde_json::from_str::<Issued>(&payload).ok())
        .filter(|issued| issued.origin == origin);
    if let Some(mut issued) = stored.clone() {
        // The station may rename its header; the credential itself is unchanged.
        issued.header.clone_from(&paths.app_token_header);
        state().issued = Some(issued);
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
        if crate::legacy::write_secret(KEYRING_SERVICE, KEYRING_ACCOUNT, &payload).is_err() {
            // Still usable until the client exits; it is asked for again next time.
            log::warn!("account stage=app_token_not_stored");
        }
    }
    log::info!("account stage=app_token_issued");
    state().issued = Some(issued);
}

/// Signing out, or signing in again, ends the credential: it was issued to
/// the account that was signed in.
pub(super) fn forget() {
    let mut state = state();
    state.issued = None;
    state.last_attempt = None;
    drop(state);
    let _ = crate::legacy::delete_secret(KEYRING_SERVICE, KEYRING_ACCOUNT);
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
