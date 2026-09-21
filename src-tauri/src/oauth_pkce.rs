//! Client-owned OAuth 2.0 Authorization Code + PKCE primitives.
//!
//! This module is intentionally not wired into the active account commands yet.
//! The server token endpoint and the bearer-to-tool billing-group contract are
//! not deployed. Keeping the protocol engine compiled but dormant lets us test
//! the client boundary now without replacing the working device-code flow.

use std::{
    collections::BTreeSet,
    net::Ipv4Addr,
    sync::atomic::{AtomicU8, Ordering},
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use reqwest::Url;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

pub(crate) const CLIENT_ID: &str = "yeschoy-desktop";
pub(crate) const CALLBACK_PATH: &str = "/oauth/callback";
pub(crate) const AUTHORIZE_PATH: &str = "/api/oauth/authorize";
pub(crate) const TOKEN_ENDPOINT: &str = "https://yeschoy.com/api/oauth/token";
pub(crate) const USERINFO_ENDPOINT: &str = "https://yeschoy.com/api/oauth/userinfo";
pub(crate) const REVOKE_ENDPOINT: &str = "https://yeschoy.com/api/oauth/revoke";
pub(crate) const DEFAULT_SCOPE: &str = "profile api offline_access";
pub(crate) const AUTHORIZATION_WAIT: Duration = Duration::from_secs(600);

const CANONICAL_AUTHORIZATION_ORIGIN: &str = "https://yeschoy.com";
const PARTNER_AUTHORIZATION_ORIGIN: &str = "https://ai.yeschoy.io";
const COMPILED_AUTHORIZATION_ORIGIN: &str = env!("YESCHOY_AUTHORIZATION_PAGE_ORIGIN");
const MAX_CALLBACK_BYTES: usize = 8 * 1024;
const CALLBACK_READ_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_TOKEN_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const PHASE_WAITING: u8 = 0;
const PHASE_CLAIMED: u8 = 1;
const PHASE_CANCELLED: u8 = 2;
const PHASE_EXPIRED: u8 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PkceFailure {
    RandomUnavailable,
    ListenerUnavailable,
    InvalidOrigin,
    InvalidRedirect,
    InvalidAuthorizationInput,
    InvalidDeviceMetadata,
    InvalidCredential,
    InvalidResponse,
    ResponseTooLarge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CallbackHttpStatus {
    BadRequest,
    NotFound,
    MethodNotAllowed,
    Conflict,
}

pub(crate) enum CallbackOutcome {
    NoRequest,
    Rejected(CallbackHttpStatus),
    AuthorizationCode(AuthorizationCode),
    AccessDenied,
    AlreadyHandled,
    Cancelled,
    Expired,
}

pub(crate) struct AuthorizationCode(String);

impl AuthorizationCode {
    fn new(value: String) -> Result<Self, PkceFailure> {
        if opaque_secret_is_valid(&value, 32, 4096) {
            Ok(Self(value))
        } else {
            Err(PkceFailure::InvalidCredential)
        }
    }

    fn expose(&self) -> &str {
        &self.0
    }
}

pub(crate) struct SensitiveFormRequest {
    endpoint: &'static str,
    body: String,
}

impl SensitiveFormRequest {
    pub(crate) fn endpoint(&self) -> &'static str {
        self.endpoint
    }

    pub(crate) fn body(&self) -> &str {
        &self.body
    }
}

pub(crate) struct OAuthTokenBundle {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
    refresh_expires_in: Option<u64>,
    scopes: BTreeSet<String>,
    session_id: String,
}

impl OAuthTokenBundle {
    pub(crate) fn access_token(&self) -> &str {
        &self.access_token
    }

    pub(crate) fn refresh_token(&self) -> Option<&str> {
        self.refresh_token.as_deref()
    }

    pub(crate) fn expires_in(&self) -> u64 {
        self.expires_in
    }

    pub(crate) fn refresh_expires_in(&self) -> Option<u64> {
        self.refresh_expires_in
    }

    pub(crate) fn scopes(&self) -> &BTreeSet<String> {
        &self.scopes
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }
}

pub(crate) struct DeviceMetadata {
    device_id: String,
    device_name: Option<String>,
    platform: &'static str,
    client_version: &'static str,
}

impl DeviceMetadata {
    pub(crate) fn new(device_id: String, device_name: Option<String>) -> Result<Self, PkceFailure> {
        if !uuid_v4_text_is_valid(&device_id)
            || device_name
                .as_deref()
                .is_some_and(|value| !bounded_unicode_text(value, 80))
            || !bounded_unicode_text(env!("CARGO_PKG_VERSION"), 64)
        {
            return Err(PkceFailure::InvalidDeviceMetadata);
        }
        Ok(Self {
            device_id,
            device_name,
            platform: platform_name(),
            client_version: env!("CARGO_PKG_VERSION"),
        })
    }

    pub(crate) fn generate(device_name: Option<String>) -> Result<Self, PkceFailure> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| PkceFailure::RandomUnavailable)?;
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let device_id = format!(
            "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
            u32::from_be_bytes(bytes[0..4].try_into().expect("four bytes")),
            u16::from_be_bytes(bytes[4..6].try_into().expect("two bytes")),
            u16::from_be_bytes(bytes[6..8].try_into().expect("two bytes")),
            u16::from_be_bytes(bytes[8..10].try_into().expect("two bytes")),
            u64::from_be_bytes([
                0, 0, bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
            ])
        );
        Self::new(device_id, device_name)
    }
}

pub(crate) struct PreparedAuthorization {
    listener: TcpListener,
    redirect_uri: String,
    browser_url: String,
    state: String,
    verifier: String,
    challenge: String,
    deadline: Instant,
    phase: AtomicU8,
}

impl PreparedAuthorization {
    pub(crate) async fn prepare() -> Result<Self, PkceFailure> {
        Self::prepare_for_origin(COMPILED_AUTHORIZATION_ORIGIN, AUTHORIZATION_WAIT).await
    }

    async fn prepare_for_origin(origin: &str, lifetime: Duration) -> Result<Self, PkceFailure> {
        if !authorization_origin_is_allowed(origin) {
            return Err(PkceFailure::InvalidOrigin);
        }

        // Hold the operating-system-assigned socket for the entire flow. Never
        // probe, close and rebind a guessed port.
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| PkceFailure::ListenerUnavailable)?;
        let address = listener
            .local_addr()
            .map_err(|_| PkceFailure::ListenerUnavailable)?;
        if address.ip() != Ipv4Addr::LOCALHOST || address.port() == 0 {
            return Err(PkceFailure::ListenerUnavailable);
        }
        let redirect_uri = canonical_redirect_uri(address.port())?;

        let state = random_base64url_32()?;
        let mut verifier = random_base64url_32()?;
        if constant_time_equal(state.as_bytes(), verifier.as_bytes()) {
            verifier = random_base64url_32()?;
            if constant_time_equal(state.as_bytes(), verifier.as_bytes()) {
                return Err(PkceFailure::RandomUnavailable);
            }
        }
        let challenge = pkce_s256(&verifier)?;
        let browser_url =
            authorization_url(origin, &redirect_uri, &state, &challenge, DEFAULT_SCOPE)?;

        Ok(Self {
            listener,
            redirect_uri,
            browser_url,
            state,
            verifier,
            challenge,
            deadline: Instant::now() + lifetime,
            phase: AtomicU8::new(PHASE_WAITING),
        })
    }

    pub(crate) fn browser_url(&self) -> &str {
        &self.browser_url
    }

    pub(crate) fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    pub(crate) fn local_port(&self) -> u16 {
        self.listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(0)
    }

    pub(crate) fn cancel(&self) -> bool {
        self.phase
            .compare_exchange(
                PHASE_WAITING,
                PHASE_CANCELLED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    pub(crate) async fn accept_one(&self, wait: Duration) -> Result<CallbackOutcome, PkceFailure> {
        match self.current_phase() {
            PHASE_CLAIMED => return Ok(CallbackOutcome::AlreadyHandled),
            PHASE_CANCELLED => return Ok(CallbackOutcome::Cancelled),
            PHASE_EXPIRED => return Ok(CallbackOutcome::Expired),
            _ => {}
        }
        if self.expire_if_needed() {
            return Ok(CallbackOutcome::Expired);
        }
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        let wait = wait.min(remaining);
        let accepted = match timeout(wait, self.listener.accept()).await {
            Ok(Ok(value)) => value,
            Ok(Err(_)) => return Err(PkceFailure::ListenerUnavailable),
            Err(_) => return Ok(CallbackOutcome::NoRequest),
        };
        let (mut stream, peer) = accepted;
        if !peer.ip().is_loopback() {
            let _ = write_callback_response(&mut stream, CallbackResponse::BadRequest).await;
            return Ok(CallbackOutcome::Rejected(CallbackHttpStatus::BadRequest));
        }
        let request = match read_callback_request(&mut stream).await {
            Ok(value) => value,
            Err(status) => {
                let response = CallbackResponse::from_status(status);
                let _ = write_callback_response(&mut stream, response).await;
                return Ok(CallbackOutcome::Rejected(status));
            }
        };
        let expected_host = format!("127.0.0.1:{}", self.local_port());
        let evaluated = evaluate_callback_request(&request, &expected_host, &self.state);
        match evaluated {
            CallbackEvaluation::Rejected(status) => {
                let response = CallbackResponse::from_status(status);
                let _ = write_callback_response(&mut stream, response).await;
                Ok(CallbackOutcome::Rejected(status))
            }
            CallbackEvaluation::AccessDenied => match self.claim_valid_callback() {
                ClaimResult::Claimed => {
                    let _ = write_callback_response(&mut stream, CallbackResponse::Denied).await;
                    Ok(CallbackOutcome::AccessDenied)
                }
                ClaimResult::AlreadyHandled => {
                    let _ = write_callback_response(&mut stream, CallbackResponse::Conflict).await;
                    Ok(CallbackOutcome::AlreadyHandled)
                }
                ClaimResult::Cancelled => {
                    let _ =
                        write_callback_response(&mut stream, CallbackResponse::BadRequest).await;
                    Ok(CallbackOutcome::Cancelled)
                }
                ClaimResult::Expired => {
                    let _ =
                        write_callback_response(&mut stream, CallbackResponse::BadRequest).await;
                    Ok(CallbackOutcome::Expired)
                }
            },
            CallbackEvaluation::AuthorizationCode(code) => match self.claim_valid_callback() {
                ClaimResult::Claimed => {
                    let code = AuthorizationCode::new(code)?;
                    let _ = write_callback_response(&mut stream, CallbackResponse::Accepted).await;
                    Ok(CallbackOutcome::AuthorizationCode(code))
                }
                ClaimResult::AlreadyHandled => {
                    let _ = write_callback_response(&mut stream, CallbackResponse::Conflict).await;
                    Ok(CallbackOutcome::AlreadyHandled)
                }
                ClaimResult::Cancelled => {
                    let _ =
                        write_callback_response(&mut stream, CallbackResponse::BadRequest).await;
                    Ok(CallbackOutcome::Cancelled)
                }
                ClaimResult::Expired => {
                    let _ =
                        write_callback_response(&mut stream, CallbackResponse::BadRequest).await;
                    Ok(CallbackOutcome::Expired)
                }
            },
        }
    }

    pub(crate) fn build_code_exchange_request(
        &self,
        code: AuthorizationCode,
        metadata: &DeviceMetadata,
    ) -> Result<SensitiveFormRequest, PkceFailure> {
        if self.current_phase() != PHASE_CLAIMED
            || !redirect_uri_is_allowed(&self.redirect_uri)
            || !pkce_verifier_is_valid(&self.verifier)
        {
            return Err(PkceFailure::InvalidAuthorizationInput);
        }
        let mut pairs = vec![
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT_ID),
            ("code", code.expose()),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("code_verifier", self.verifier.as_str()),
            ("device_id", metadata.device_id.as_str()),
        ];
        if let Some(name) = metadata.device_name.as_deref() {
            pairs.push(("device_name", name));
        }
        pairs.extend([
            ("platform", metadata.platform),
            ("client_version", metadata.client_version),
        ]);
        Ok(SensitiveFormRequest {
            endpoint: TOKEN_ENDPOINT,
            body: form_encode(&pairs),
        })
    }

    fn current_phase(&self) -> u8 {
        self.phase.load(Ordering::Acquire)
    }

    fn expire_if_needed(&self) -> bool {
        if Instant::now() < self.deadline {
            return self.current_phase() == PHASE_EXPIRED;
        }
        let _ = self.phase.compare_exchange(
            PHASE_WAITING,
            PHASE_EXPIRED,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        self.current_phase() == PHASE_EXPIRED
    }

    fn claim_valid_callback(&self) -> ClaimResult {
        if self.expire_if_needed() {
            return ClaimResult::Expired;
        }
        match self.phase.compare_exchange(
            PHASE_WAITING,
            PHASE_CLAIMED,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => ClaimResult::Claimed,
            Err(PHASE_CANCELLED) => ClaimResult::Cancelled,
            Err(PHASE_EXPIRED) => ClaimResult::Expired,
            Err(_) => ClaimResult::AlreadyHandled,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClaimResult {
    Claimed,
    AlreadyHandled,
    Cancelled,
    Expired,
}

enum CallbackEvaluation {
    Rejected(CallbackHttpStatus),
    AuthorizationCode(String),
    AccessDenied,
}

#[derive(Clone, Copy)]
enum CallbackResponse {
    Accepted,
    Denied,
    BadRequest,
    NotFound,
    MethodNotAllowed,
    Conflict,
}

impl CallbackResponse {
    fn from_status(status: CallbackHttpStatus) -> Self {
        match status {
            CallbackHttpStatus::BadRequest => Self::BadRequest,
            CallbackHttpStatus::NotFound => Self::NotFound,
            CallbackHttpStatus::MethodNotAllowed => Self::MethodNotAllowed,
            CallbackHttpStatus::Conflict => Self::Conflict,
        }
    }

    fn status_line(self) -> &'static str {
        match self {
            Self::Accepted | Self::Denied => "200 OK",
            Self::BadRequest => "400 Bad Request",
            Self::NotFound => "404 Not Found",
            Self::MethodNotAllowed => "405 Method Not Allowed",
            Self::Conflict => "409 Conflict",
        }
    }

    // The browser tab is the only thing the user sees at this moment, so it
    // has to speak the interface language. Takes the language explicitly so
    // tests can pin one instead of racing on the process-wide setting.
    fn message_in(self, language: crate::ui_language::Language) -> &'static str {
        match self {
            Self::Accepted => language.pick(
                "授权回调已接收，请返回野菜客户端查看登录结果。",
                "Sign-in received. Return to the Yeschoy app to see the result.",
            ),
            Self::Denied => language.pick(
                "授权已取消，请返回野菜客户端。",
                "Sign-in cancelled. Return to the Yeschoy app.",
            ),
            Self::BadRequest => language.pick(
                "请求无效，请返回客户端重试。",
                "Invalid request. Return to the app and try again.",
            ),
            Self::NotFound => language.pick("页面不存在。", "Page not found."),
            Self::MethodNotAllowed => {
                language.pick("请求方法不受支持。", "Request method not supported.")
            }
            Self::Conflict => language.pick(
                "该授权回调已经处理。",
                "This sign-in has already been handled.",
            ),
        }
    }

    fn page_in(self, language: crate::ui_language::Language) -> String {
        format!(
            "<!doctype html><html lang=\"{}\"><meta charset=\"utf-8\"><title>{}</title><body><p>{}</p></body></html>",
            language.pick("zh-CN", "en"),
            language.pick("野菜API 授权", "Yeschoy sign-in"),
            self.message_in(language)
        )
    }
}

pub(crate) fn build_refresh_request(
    refresh_token: &str,
) -> Result<SensitiveFormRequest, PkceFailure> {
    if !opaque_secret_is_valid(refresh_token, 32, 8192) {
        return Err(PkceFailure::InvalidCredential);
    }
    Ok(SensitiveFormRequest {
        endpoint: TOKEN_ENDPOINT,
        body: form_encode(&[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", refresh_token),
        ]),
    })
}

pub(crate) fn build_revoke_request(
    token: &str,
    token_type_hint: &str,
) -> Result<SensitiveFormRequest, PkceFailure> {
    if !opaque_secret_is_valid(token, 32, 8192)
        || !matches!(token_type_hint, "refresh_token" | "access_token")
    {
        return Err(PkceFailure::InvalidCredential);
    }
    Ok(SensitiveFormRequest {
        endpoint: REVOKE_ENDPOINT,
        body: form_encode(&[
            ("token", token),
            ("client_id", CLIENT_ID),
            ("token_type_hint", token_type_hint),
        ]),
    })
}

pub(crate) fn parse_token_response(
    status: u16,
    content_type: &str,
    body: &[u8],
) -> Result<OAuthTokenBundle, PkceFailure> {
    if body.len() > MAX_TOKEN_RESPONSE_BYTES {
        return Err(PkceFailure::ResponseTooLarge);
    }
    if !content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .eq_ignore_ascii_case("application/json")
    {
        return Err(PkceFailure::InvalidResponse);
    }
    if status != 200 {
        let error: OAuthErrorResponse =
            serde_json::from_slice(body).map_err(|_| PkceFailure::InvalidResponse)?;
        if !oauth_identifier_is_valid(&error.error)
            || error
                .error_description
                .as_deref()
                .is_some_and(|value| !bounded_unicode_text(value, 512))
            || error
                .request_id
                .as_deref()
                .is_some_and(|value| !opaque_identifier_is_valid(value, 128))
        {
            return Err(PkceFailure::InvalidResponse);
        }
        return Err(PkceFailure::InvalidCredential);
    }

    let response: OAuthTokenResponse =
        serde_json::from_slice(body).map_err(|_| PkceFailure::InvalidResponse)?;
    if response.token_type != "Bearer"
        || !opaque_secret_is_valid(&response.access_token, 32, 8192)
        || response.expires_in == 0
        || response.expires_in > 3600
        || !opaque_identifier_is_valid(&response.session_id, 256)
    {
        return Err(PkceFailure::InvalidResponse);
    }
    let scopes = parse_scope(&response.scope)?;
    if !scopes.contains("profile") || !scopes.contains("api") {
        return Err(PkceFailure::InvalidResponse);
    }
    let has_offline = scopes.contains("offline_access");
    if has_offline != (response.refresh_token.is_some() && response.refresh_expires_in.is_some()) {
        return Err(PkceFailure::InvalidResponse);
    }
    if response
        .refresh_token
        .as_deref()
        .is_some_and(|value| !opaque_secret_is_valid(value, 32, 8192))
        || response
            .refresh_expires_in
            .is_some_and(|seconds| seconds <= response.expires_in || seconds > 30 * 24 * 60 * 60)
    {
        return Err(PkceFailure::InvalidResponse);
    }
    Ok(OAuthTokenBundle {
        access_token: response.access_token,
        refresh_token: response.refresh_token,
        expires_in: response.expires_in,
        refresh_expires_in: response.refresh_expires_in,
        scopes,
        session_id: response.session_id,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OAuthTokenResponse {
    access_token: String,
    token_type: String,
    expires_in: u64,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    refresh_expires_in: Option<u64>,
    scope: String,
    session_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OAuthErrorResponse {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
    #[serde(default)]
    request_id: Option<String>,
}

fn authorization_origin_is_allowed(raw: &str) -> bool {
    if !matches!(
        raw,
        CANONICAL_AUTHORIZATION_ORIGIN | PARTNER_AUTHORIZATION_ORIGIN
    ) {
        return false;
    }
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    url.scheme() == "https"
        && matches!(url.host_str(), Some("yeschoy.com" | "ai.yeschoy.io"))
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none()
}

fn canonical_redirect_uri(port: u16) -> Result<String, PkceFailure> {
    if port == 0 {
        return Err(PkceFailure::InvalidRedirect);
    }
    let value = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
    if redirect_uri_is_allowed(&value) {
        Ok(value)
    } else {
        Err(PkceFailure::InvalidRedirect)
    }
}

fn redirect_uri_is_allowed(raw: &str) -> bool {
    let Some(authority_and_path) = raw.strip_prefix("http://127.0.0.1:") else {
        return false;
    };
    let Some((port_text, path)) = authority_and_path.split_once('/') else {
        return false;
    };
    if port_text.is_empty()
        || (port_text.len() > 1 && port_text.starts_with('0'))
        || port_text
            .parse::<u16>()
            .ok()
            .filter(|port| *port > 0)
            .is_none()
        || format!("/{path}") != CALLBACK_PATH
    {
        return false;
    }
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path() == CALLBACK_PATH
}

fn authorization_url(
    origin: &str,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
    scope: &str,
) -> Result<String, PkceFailure> {
    if !authorization_origin_is_allowed(origin)
        || !redirect_uri_is_allowed(redirect_uri)
        || !base64url_43_is_valid(state)
        || !base64url_43_is_valid(challenge)
        || !scope_is_requestable(scope)
    {
        return Err(PkceFailure::InvalidAuthorizationInput);
    }
    let mut url =
        Url::parse(&format!("{origin}{AUTHORIZE_PATH}")).map_err(|_| PkceFailure::InvalidOrigin)?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("scope", scope);
    Ok(url.into())
}

fn random_base64url_32() -> Result<String, PkceFailure> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| PkceFailure::RandomUnavailable)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn pkce_s256(verifier: &str) -> Result<String, PkceFailure> {
    if !pkce_verifier_is_valid(verifier) {
        return Err(PkceFailure::InvalidAuthorizationInput);
    }
    Ok(URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())))
}

fn pkce_verifier_is_valid(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
}

fn base64url_43_is_valid(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn scope_is_requestable(value: &str) -> bool {
    value == DEFAULT_SCOPE || value == "profile api offline_access sessions"
}

fn parse_scope(value: &str) -> Result<BTreeSet<String>, PkceFailure> {
    let mut scopes = BTreeSet::new();
    for scope in value.split(' ') {
        if scope.is_empty()
            || !matches!(scope, "profile" | "api" | "offline_access" | "sessions")
            || !scopes.insert(scope.to_owned())
        {
            return Err(PkceFailure::InvalidResponse);
        }
    }
    Ok(scopes)
}

fn form_encode(pairs: &[(&str, &str)]) -> String {
    let mut url = Url::parse("https://form.invalid/").expect("constant form URL is valid");
    {
        let mut query = url.query_pairs_mut();
        for (name, value) in pairs {
            query.append_pair(name, value);
        }
    }
    url.query().unwrap_or_default().to_owned()
}

async fn read_callback_request(stream: &mut TcpStream) -> Result<Vec<u8>, CallbackHttpStatus> {
    let mut request = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let count = match timeout(CALLBACK_READ_TIMEOUT, stream.read(&mut chunk)).await {
            Ok(Ok(value)) => value,
            _ => return Err(CallbackHttpStatus::BadRequest),
        };
        if count == 0 {
            return Err(CallbackHttpStatus::BadRequest);
        }
        if count > MAX_CALLBACK_BYTES.saturating_sub(request.len()) {
            return Err(CallbackHttpStatus::BadRequest);
        }
        request.extend_from_slice(&chunk[..count]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(request);
        }
    }
}

fn evaluate_callback_request(
    request: &[u8],
    expected_host: &str,
    expected_state: &str,
) -> CallbackEvaluation {
    let Ok(text) = std::str::from_utf8(request) else {
        return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest);
    };
    let Some(head_end) = text.find("\r\n\r\n") else {
        return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest);
    };
    let mut lines = text[..head_end].split("\r\n");
    let Some(request_line) = lines.next() else {
        return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest);
    };
    let mut request_parts = request_line.split(' ');
    let Some(method) = request_parts.next() else {
        return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest);
    };
    let Some(target) = request_parts.next() else {
        return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest);
    };
    let Some(version) = request_parts.next() else {
        return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest);
    };
    if request_parts.next().is_some() || version != "HTTP/1.1" {
        return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest);
    }
    if method != "GET" {
        return CallbackEvaluation::Rejected(CallbackHttpStatus::MethodNotAllowed);
    }

    let mut host = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest);
        };
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || value.chars().any(char::is_control)
        {
            return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest);
        }
        if name.eq_ignore_ascii_case("host") && host.replace(value.trim()).is_some() {
            return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest);
        }
    }
    if host != Some(expected_host) {
        return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest);
    }
    if target.starts_with("//") || target.contains('#') || !valid_percent_encoding(target) {
        return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest);
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != CALLBACK_PATH {
        return CallbackEvaluation::Rejected(CallbackHttpStatus::NotFound);
    }
    if query.is_empty() {
        return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest);
    }
    let Ok(parsed) = Url::parse(&format!("https://callback.invalid/?{query}")) else {
        return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest);
    };
    let mut state = None;
    let mut code = None;
    let mut error = None;
    let mut error_description = None;
    for (name, value) in parsed.query_pairs() {
        let slot = match name.as_ref() {
            "state" => &mut state,
            "code" => &mut code,
            "error" => &mut error,
            "error_description" => &mut error_description,
            _ => return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest),
        };
        if slot.replace(value.into_owned()).is_some() {
            return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest);
        }
    }
    let Some(state) = state else {
        return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest);
    };
    if !constant_time_equal(state.as_bytes(), expected_state.as_bytes()) {
        return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest);
    }
    if code.is_some() == error.is_some() {
        return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest);
    }
    if let Some(code) = code {
        if error_description.is_some() || !opaque_secret_is_valid(&code, 32, 4096) {
            return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest);
        }
        return CallbackEvaluation::AuthorizationCode(code);
    }
    let error = error.expect("exclusive result checked");
    if !oauth_identifier_is_valid(&error)
        || error_description
            .as_deref()
            .is_some_and(|value| !bounded_unicode_text(value, 512))
    {
        return CallbackEvaluation::Rejected(CallbackHttpStatus::BadRequest);
    }
    CallbackEvaluation::AccessDenied
}

async fn write_callback_response(
    stream: &mut TcpStream,
    response: CallbackResponse,
) -> std::io::Result<()> {
    let body = response.page_in(crate::ui_language::current());
    let allow = if matches!(response, CallbackResponse::MethodNotAllowed) {
        "Allow: GET\r\n"
    } else {
        ""
    };
    let head = format!(
        "HTTP/1.1 {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nPragma: no-cache\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; base-uri 'none'; frame-ancestors 'none'\r\nX-Content-Type-Options: nosniff\r\n{}Connection: close\r\n\r\n",
        response.status_line(),
        body.len(),
        allow
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    stream.shutdown().await
}

fn valid_percent_encoding(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    true
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

fn opaque_secret_is_valid(value: &str, minimum: usize, maximum: usize) -> bool {
    (minimum..=maximum).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'"' | b'\\'))
}

fn opaque_identifier_is_valid(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn oauth_identifier_is_valid(value: &str) -> bool {
    opaque_identifier_is_valid(value, 64)
}

fn bounded_unicode_text(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.chars().count() <= maximum
        && !value.chars().any(|character| {
            character.is_control()
                || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

fn uuid_v4_text_is_valid(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }
    value.bytes().enumerate().all(|(index, byte)| match index {
        8 | 13 | 18 | 23 => byte == b'-',
        14 => byte == b'4',
        19 => matches!(byte.to_ascii_lowercase(), b'8' | b'9' | b'a' | b'b'),
        _ => byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase(),
    })
}

const fn platform_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RFC_VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    const RFC_CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
    const TEST_CODE: &str = "authorization-code-0123456789-ABCDEFG";
    const TEST_ACCESS: &str = "access-token-0123456789-ABCDEFGHIJK";
    const TEST_REFRESH: &str = "refresh-token-0123456789-ABCDEFGHIJ";

    #[test]
    fn rfc_7636_s256_vector_matches() {
        assert_eq!(pkce_s256(RFC_VERIFIER).unwrap(), RFC_CHALLENGE);
    }

    #[tokio::test]
    async fn preparation_binds_before_building_the_exact_authorization_url() {
        let prepared = PreparedAuthorization::prepare_for_origin(
            PARTNER_AUTHORIZATION_ORIGIN,
            AUTHORIZATION_WAIT,
        )
        .await
        .unwrap();
        assert!(prepared.local_port() > 0);
        assert_eq!(
            prepared.redirect_uri(),
            format!("http://127.0.0.1:{}/oauth/callback", prepared.local_port())
        );
        assert!(prepared
            .browser_url()
            .starts_with("https://ai.yeschoy.io/api/oauth/authorize?"));
        let url = Url::parse(prepared.browser_url()).unwrap();
        let query = url.query_pairs().collect::<Vec<_>>();
        assert!(query
            .iter()
            .any(|(name, value)| { name == "redirect_uri" && value == prepared.redirect_uri() }));
        assert!(query
            .iter()
            .any(|(name, value)| name == "code_challenge" && value == &prepared.challenge));
        assert_ne!(prepared.state, prepared.verifier);
    }

    #[test]
    fn origins_redirects_and_authorization_inputs_are_closed() {
        assert!(authorization_origin_is_allowed(
            CANONICAL_AUTHORIZATION_ORIGIN
        ));
        assert!(authorization_origin_is_allowed(
            PARTNER_AUTHORIZATION_ORIGIN
        ));
        for rejected in [
            "http://yeschoy.com",
            "https://yeschoy.com/authorize",
            "https://ai.yeschoy.io:443",
            "https://ai.yeschoy.io.attacker.invalid",
        ] {
            assert!(!authorization_origin_is_allowed(rejected));
        }
        assert!(redirect_uri_is_allowed(
            "http://127.0.0.1:49182/oauth/callback"
        ));
        for rejected in [
            "http://127.0.0.1:0/oauth/callback",
            "http://127.0.0.1:049182/oauth/callback",
            "http://localhost:49182/oauth/callback",
            "http://0.0.0.0:49182/oauth/callback",
            "http://127.0.0.1:49182/oauth/callback/",
            "http://127.0.0.1:49182/oauth/callback?next=x",
        ] {
            assert!(!redirect_uri_is_allowed(rejected), "accepted {rejected}");
        }
    }

    #[test]
    fn callback_page_speaks_the_interface_language() {
        use crate::ui_language::Language;
        let english = CallbackResponse::Accepted.page_in(Language::En);
        assert!(english.starts_with("<!doctype html><html lang=\"en\">"));
        assert!(english.contains("<title>Yeschoy sign-in</title>"));
        assert!(english.contains("Sign-in received. Return to the Yeschoy app to see the result."));
        // Not one Han character may leak onto an English user's screen.
        assert!(english.is_ascii(), "{english}");
        for response in [
            CallbackResponse::Denied,
            CallbackResponse::BadRequest,
            CallbackResponse::NotFound,
            CallbackResponse::MethodNotAllowed,
            CallbackResponse::Conflict,
        ] {
            assert!(response.page_in(Language::En).is_ascii());
        }

        let chinese = CallbackResponse::Accepted.page_in(Language::Zh);
        assert!(chinese.starts_with("<!doctype html><html lang=\"zh-CN\">"));
        assert!(chinese.contains("<title>野菜API 授权</title>"));
        assert!(chinese.contains("授权回调已接收，请返回野菜客户端查看登录结果。"));
    }

    #[tokio::test]
    async fn wrong_state_does_not_consume_then_valid_callback_claims_once() {
        let prepared = PreparedAuthorization::prepare_for_origin(
            CANONICAL_AUTHORIZATION_ORIGIN,
            AUTHORIZATION_WAIT,
        )
        .await
        .unwrap();
        let wrong = format!(
            "GET /oauth/callback?code={TEST_CODE}&state={} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
            "A".repeat(43),
            prepared.local_port()
        );
        let (outcome, response) = send_and_accept(&prepared, &wrong).await;
        assert!(matches!(
            outcome,
            CallbackOutcome::Rejected(CallbackHttpStatus::BadRequest)
        ));
        assert!(response.starts_with("HTTP/1.1 400"));
        assert_eq!(prepared.current_phase(), PHASE_WAITING);

        let valid = callback_request(&prepared, TEST_CODE);
        let (outcome, response) = send_and_accept(&prepared, &valid).await;
        let code = match outcome {
            CallbackOutcome::AuthorizationCode(code) => code,
            _ => panic!("valid callback did not yield a code"),
        };
        assert_eq!(code.expose(), TEST_CODE);
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(!response.contains(TEST_CODE));
        assert!(!response.contains(&prepared.state));
        assert_eq!(prepared.current_phase(), PHASE_CLAIMED);
        assert_eq!(prepared.claim_valid_callback(), ClaimResult::AlreadyHandled);
    }

    #[test]
    fn callback_parser_rejects_ambiguous_and_foreign_requests() {
        let state = "S".repeat(43);
        let host = "127.0.0.1:49182";
        let cases = [
            format!("POST /oauth/callback?code={TEST_CODE}&state={state} HTTP/1.1\r\nHost: {host}\r\n\r\n"),
            format!("GET /other?code={TEST_CODE}&state={state} HTTP/1.1\r\nHost: {host}\r\n\r\n"),
            format!("GET /oauth/callback?code={TEST_CODE}&state={state} HTTP/1.1\r\nHost: attacker.invalid\r\n\r\n"),
            format!("GET /oauth/callback?code={TEST_CODE}&code={TEST_CODE}&state={state} HTTP/1.1\r\nHost: {host}\r\n\r\n"),
            format!("GET /oauth/callback?code={TEST_CODE}&error=access_denied&state={state} HTTP/1.1\r\nHost: {host}\r\n\r\n"),
            format!("GET /oauth/callback?code={TEST_CODE}&state={state}&next=x HTTP/1.1\r\nHost: {host}\r\n\r\n"),
        ];
        for request in cases {
            assert!(matches!(
                evaluate_callback_request(request.as_bytes(), host, &state),
                CallbackEvaluation::Rejected(_)
            ));
        }
    }

    #[tokio::test]
    async fn cancellation_and_expiry_block_claims() {
        let cancelled = PreparedAuthorization::prepare_for_origin(
            CANONICAL_AUTHORIZATION_ORIGIN,
            AUTHORIZATION_WAIT,
        )
        .await
        .unwrap();
        assert!(cancelled.cancel());
        assert_eq!(cancelled.claim_valid_callback(), ClaimResult::Cancelled);

        let expired = PreparedAuthorization::prepare_for_origin(
            CANONICAL_AUTHORIZATION_ORIGIN,
            Duration::ZERO,
        )
        .await
        .unwrap();
        assert_eq!(expired.claim_valid_callback(), ClaimResult::Expired);
    }

    #[tokio::test]
    async fn exchange_refresh_and_revoke_forms_are_fixed_and_encoded() {
        let prepared = PreparedAuthorization::prepare_for_origin(
            CANONICAL_AUTHORIZATION_ORIGIN,
            AUTHORIZATION_WAIT,
        )
        .await
        .unwrap();
        assert_eq!(prepared.claim_valid_callback(), ClaimResult::Claimed);
        let metadata = DeviceMetadata::new(
            "550e8400-e29b-41d4-a716-446655440000".into(),
            Some("我的 MacBook".into()),
        )
        .unwrap();
        let request = prepared
            .build_code_exchange_request(
                AuthorizationCode::new(TEST_CODE.into()).unwrap(),
                &metadata,
            )
            .unwrap();
        assert_eq!(request.endpoint(), TOKEN_ENDPOINT);
        assert!(request.body().contains("grant_type=authorization_code"));
        assert!(request.body().contains("client_id=yeschoy-desktop"));
        assert!(request
            .body()
            .contains("device_name=%E6%88%91%E7%9A%84+MacBook"));
        assert!(!request.body().contains("client_secret"));

        let refresh = build_refresh_request(TEST_REFRESH).unwrap();
        assert_eq!(refresh.endpoint(), TOKEN_ENDPOINT);
        assert!(refresh.body().contains("grant_type=refresh_token"));
        let revoke = build_revoke_request(TEST_REFRESH, "refresh_token").unwrap();
        assert_eq!(revoke.endpoint(), REVOKE_ENDPOINT);
        assert!(revoke.body().contains("token_type_hint=refresh_token"));
    }

    #[test]
    fn token_response_requires_bounded_scoped_rotation_shape() {
        let body = format!(
            r#"{{"access_token":"{TEST_ACCESS}","token_type":"Bearer","expires_in":3600,"refresh_token":"{TEST_REFRESH}","refresh_expires_in":2592000,"scope":"profile api offline_access","session_id":"sess_a82f9c"}}"#
        );
        let bundle =
            parse_token_response(200, "application/json; charset=utf-8", body.as_bytes()).unwrap();
        assert_eq!(bundle.access_token(), TEST_ACCESS);
        assert_eq!(bundle.refresh_token(), Some(TEST_REFRESH));
        assert_eq!(bundle.expires_in(), 3600);
        assert_eq!(bundle.refresh_expires_in(), Some(2_592_000));
        assert!(bundle.scopes().contains("profile"));
        assert_eq!(bundle.session_id(), "sess_a82f9c");

        for invalid in [
            body.replace("Bearer", "bearer"),
            body.replace("profile api offline_access", "profile offline_access"),
            body.replace("2592000", "3600"),
            body.replace("\"session_id\":", "\"unknown\":1,\"session_id\":"),
        ] {
            assert_eq!(
                parse_token_response(200, "application/json", invalid.as_bytes()).err(),
                Some(PkceFailure::InvalidResponse)
            );
        }
        assert_eq!(
            parse_token_response(
                400,
                "application/json",
                br#"{"error":"invalid_grant","error_description":"expired","request_id":"req_1"}"#,
            )
            .err(),
            Some(PkceFailure::InvalidCredential)
        );
    }

    #[test]
    fn generated_device_id_is_lowercase_uuid_v4() {
        let metadata = DeviceMetadata::generate(None).unwrap();
        assert!(uuid_v4_text_is_valid(&metadata.device_id));
    }

    async fn send_and_accept(
        prepared: &PreparedAuthorization,
        request: &str,
    ) -> (CallbackOutcome, String) {
        let client = async {
            let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, prepared.local_port()))
                .await
                .unwrap();
            stream.write_all(request.as_bytes()).await.unwrap();
            let mut response = Vec::new();
            stream.read_to_end(&mut response).await.unwrap();
            String::from_utf8(response).unwrap()
        };
        let server = prepared.accept_one(Duration::from_secs(1));
        let (response, outcome) = tokio::join!(client, server);
        (outcome.unwrap(), response)
    }

    fn callback_request(prepared: &PreparedAuthorization, code: &str) -> String {
        format!(
            "GET /oauth/callback?code={code}&state={} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAccept: text/html\r\n\r\n",
            prepared.state,
            prepared.local_port()
        )
    }
}
