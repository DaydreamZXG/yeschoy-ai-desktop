use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{header, Request, Response, StatusCode},
    routing::any,
    Router,
};
use serde_json::{json, Map, Value};
use tokio::{
    net::TcpListener,
    sync::{broadcast, oneshot, Mutex},
    task::JoinHandle,
    time::timeout,
};

use crate::{
    tool_adapters::{
        common::{self, ConfigFailure, FileTransaction},
        AdapterFailure, ResolvedInstallation,
    },
    tool_credentials::{self, ToolCredential},
};

const PROFILE_ID: &str = "00000000-0000-4000-8000-000000157220";
const PROFILE_NAME: &str = "野菜API";
const PROXY_ADDRESS: &str = "127.0.0.1:15729";
const PROXY_BASE: &str = "http://127.0.0.1:15729/claude-desktop";
const SAFE_ROUTE_MODEL: &str = "claude-sonnet-4-6";
const MAX_PROXY_BODY: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct VerificationEvent {
    model: String,
}

struct ProxyState {
    credential: ToolCredential,
    local_token: String,
    client: reqwest::Client,
    events: broadcast::Sender<VerificationEvent>,
}

struct ProxyRuntime {
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
    events: broadcast::Sender<VerificationEvent>,
}

#[derive(Clone, Default)]
pub(crate) struct ClaudeDesktopRuntimeState {
    runtime: Arc<Mutex<Option<ProxyRuntime>>>,
}

pub(crate) struct Prepared {
    transaction: FileTransaction,
    normal_config_path: PathBuf,
    threep_config_path: PathBuf,
    profile_path: PathBuf,
    meta_path: PathBuf,
    model: String,
    local_token: String,
}

fn config_error(error: ConfigFailure) -> AdapterFailure {
    match error {
        ConfigFailure::ExternalChange => AdapterFailure::ExternalOverride,
        ConfigFailure::Read => AdapterFailure::ConfigurationFailed("configuration_read_failed"),
        ConfigFailure::Parse => AdapterFailure::ConfigurationFailed("configuration_parse_failed"),
        ConfigFailure::Write => AdapterFailure::ConfigurationFailed("configuration_write_failed"),
        ConfigFailure::Readback => {
            AdapterFailure::ConfigurationFailed("configuration_readback_failed")
        }
        ConfigFailure::Rollback => {
            AdapterFailure::ConfigurationFailed("configuration_rollback_failed")
        }
    }
}

fn json_document(existing: Option<&[u8]>) -> Result<Value, ()> {
    match existing {
        Some(bytes) if !bytes.is_empty() => {
            let value: Value = serde_json::from_slice(bytes).map_err(|_| ())?;
            value.is_object().then_some(value).ok_or(())
        }
        _ => Ok(Value::Object(Map::new())),
    }
}

fn pretty(value: &Value) -> Result<Vec<u8>, ()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|_| ())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn deployment_config(existing: Option<&[u8]>) -> Result<Vec<u8>, ()> {
    let mut value = json_document(existing)?;
    value
        .as_object_mut()
        .ok_or(())?
        .insert("deploymentMode".into(), "3p".into());
    pretty(&value)
}

fn profile_config(model: &str, local_token: &str) -> Result<Vec<u8>, ()> {
    pretty(&json!({
        "coworkEgressAllowedHosts": ["*"],
        "disableDeploymentModeChooser": true,
        "inferenceGatewayApiKey": local_token,
        "inferenceGatewayAuthScheme": "bearer",
        "inferenceGatewayBaseUrl": PROXY_BASE,
        "inferenceProvider": "gateway",
        "inferenceModels": [{
            "name": SAFE_ROUTE_MODEL,
            "labelOverride": model,
            "supports1m": false
        }]
    }))
}

fn meta_config(existing: Option<&[u8]>) -> Result<Vec<u8>, ()> {
    let mut value = json_document(existing)?;
    let object = value.as_object_mut().ok_or(())?;
    let mut entries = object
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    entries.retain(|entry| entry.get("id").and_then(Value::as_str) != Some(PROFILE_ID));
    entries.push(json!({"id": PROFILE_ID, "name": PROFILE_NAME}));
    object.insert("entries".into(), Value::Array(entries));
    object.insert("appliedId".into(), PROFILE_ID.into());
    pretty(&value)
}

fn current_paths(home: &Path) -> Result<(PathBuf, PathBuf, PathBuf, PathBuf), AdapterFailure> {
    #[cfg(target_os = "macos")]
    let (normal, threep) = {
        let support = home.join("Library").join("Application Support");
        (support.join("Claude"), support.join("Claude-3p"))
    };
    #[cfg(target_os = "windows")]
    let (normal, threep) = {
        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Local"));
        (local.join("Claude"), local.join("Claude-3p"))
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return Err(AdapterFailure::UnsupportedProfile);

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let library = threep.join("configLibrary");
        Ok((
            normal.join("claude_desktop_config.json"),
            threep.join("claude_desktop_config.json"),
            library.join(format!("{PROFILE_ID}.json")),
            library.join("_meta.json"),
        ))
    }
}

fn new_local_token() -> Result<String, AdapterFailure> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| AdapterFailure::SecureStorageUnavailable)?;
    let mut value = String::with_capacity(68);
    value.push_str("ycg-");
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut value, "{byte:02x}").map_err(|_| AdapterFailure::SecureStorageUnavailable)?;
    }
    Ok(value)
}

pub(crate) fn prepare(
    home: &Path,
    model: &str,
    existing_local_token: Option<&str>,
) -> Result<Prepared, AdapterFailure> {
    let (normal_config_path, threep_config_path, profile_path, meta_path) = current_paths(home)?;
    let normal_before = common::snapshot(&normal_config_path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let threep_before = common::snapshot(&threep_config_path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let profile_before = common::snapshot(&profile_path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let meta_before = common::snapshot(&meta_path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let local_token = existing_local_token
        .filter(|value| value.starts_with("ycg-") && value.len() == 68)
        .map(str::to_owned)
        .map(Ok)
        .unwrap_or_else(new_local_token)?;
    let mut transaction = FileTransaction::stage_with_snapshot(
        normal_config_path.clone(),
        normal_before.clone(),
        deployment_config(normal_before.as_deref())
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?,
    )
    .map_err(config_error)?;
    transaction
        .push_with_snapshot(
            threep_config_path.clone(),
            threep_before.clone(),
            deployment_config(threep_before.as_deref())
                .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?,
        )
        .map_err(config_error)?;
    transaction
        .push_with_snapshot(
            profile_path.clone(),
            profile_before,
            profile_config(model, &local_token)
                .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?,
        )
        .map_err(config_error)?;
    transaction
        .push_with_snapshot(
            meta_path.clone(),
            meta_before.clone(),
            meta_config(meta_before.as_deref())
                .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?,
        )
        .map_err(config_error)?;
    Ok(Prepared {
        transaction,
        normal_config_path,
        threep_config_path,
        profile_path,
        meta_path,
        model: model.to_owned(),
        local_token,
    })
}

impl Prepared {
    pub(crate) fn local_token(&self) -> &str {
        &self.local_token
    }

    pub(crate) fn commit(&mut self) -> Result<(), AdapterFailure> {
        self.transaction.commit().map_err(config_error)?;
        let normal: Value = serde_json::from_slice(
            &common::snapshot(&self.normal_config_path)
                .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?
                .ok_or(AdapterFailure::ConfigurationFailed(
                    "configuration_readback_failed",
                ))?,
        )
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
        let threep: Value = serde_json::from_slice(
            &common::snapshot(&self.threep_config_path)
                .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?
                .ok_or(AdapterFailure::ConfigurationFailed(
                    "configuration_readback_failed",
                ))?,
        )
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
        let profile: Value = serde_json::from_slice(
            &common::snapshot(&self.profile_path)
                .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?
                .ok_or(AdapterFailure::ConfigurationFailed(
                    "configuration_readback_failed",
                ))?,
        )
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
        let meta: Value = serde_json::from_slice(
            &common::snapshot(&self.meta_path)
                .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?
                .ok_or(AdapterFailure::ConfigurationFailed(
                    "configuration_readback_failed",
                ))?,
        )
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
        let correct = normal["deploymentMode"].as_str() == Some("3p")
            && threep["deploymentMode"].as_str() == Some("3p")
            && profile["inferenceGatewayBaseUrl"].as_str() == Some(PROXY_BASE)
            && profile["inferenceGatewayApiKey"].as_str() == Some(&self.local_token)
            && profile["inferenceModels"][0]["name"].as_str() == Some(SAFE_ROUTE_MODEL)
            && profile["inferenceModels"][0]["labelOverride"].as_str() == Some(&self.model)
            && meta["appliedId"].as_str() == Some(PROFILE_ID);
        if correct {
            Ok(())
        } else {
            Err(AdapterFailure::ConfigurationFailed(
                "configuration_readback_failed",
            ))
        }
    }

    pub(crate) fn rollback(&mut self) -> Result<(), AdapterFailure> {
        self.transaction.rollback().map_err(config_error)
    }
}

fn unauthorized() -> Response<Body> {
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .body(Body::from("unauthorized"))
        .expect("static response")
}

async fn proxy_request(
    State(state): State<Arc<ProxyState>>,
    request: Request<Body>,
) -> Response<Body> {
    let authorization = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let expected = format!("Bearer {}", state.local_token);
    if authorization != Some(expected.as_str()) {
        return unauthorized();
    }
    let path = request.uri().path().to_owned();
    let Some(upstream_path) = path.strip_prefix("/claude-desktop") else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .expect("static response");
    };
    let upstream_path = upstream_path.to_owned();
    let is_message = upstream_path.ends_with("/v1/messages") || upstream_path == "/v1/messages";
    let method = request.method().clone();
    let anthropic_version = request.headers().get("anthropic-version").cloned();
    let accept = request.headers().get(header::ACCEPT).cloned();
    let body = match to_bytes(request.into_body(), MAX_PROXY_BODY).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return Response::builder()
                .status(StatusCode::PAYLOAD_TOO_LARGE)
                .body(Body::empty())
                .expect("static response")
        }
    };
    let mut outbound_body = body.to_vec();
    if is_message {
        let mut value: Value = match serde_json::from_slice(&outbound_body) {
            Ok(value) => value,
            Err(_) => {
                return Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Body::empty())
                    .expect("static response")
            }
        };
        let Some(object) = value.as_object_mut() else {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::empty())
                .expect("static response");
        };
        object.insert("model".into(), state.credential.model_id.clone().into());
        outbound_body = match serde_json::to_vec(&value) {
            Ok(bytes) => bytes,
            Err(_) => {
                return Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Body::empty())
                    .expect("static response")
            }
        };
    }
    let url = format!(
        "{}{}",
        state.credential.origin.trim_end_matches('/'),
        upstream_path
    );
    let mut outbound = state
        .client
        .request(method, url)
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", state.credential.api_key),
        )
        .header("x-api-key", &state.credential.api_key)
        .body(outbound_body);
    if let Some(value) = anthropic_version {
        outbound = outbound.header("anthropic-version", value);
    }
    if let Some(value) = accept {
        outbound = outbound.header(header::ACCEPT, value);
    }
    let upstream = match outbound.send().await {
        Ok(response) => response,
        Err(_) => {
            return Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::empty())
                .expect("static response")
        }
    };
    let status = upstream.status();
    let content_type = upstream.headers().get(header::CONTENT_TYPE).cloned();
    if is_message && status.is_success() {
        let _ = state.events.send(VerificationEvent {
            model: state.credential.model_id.clone(),
        });
    }
    let stream = upstream.bytes_stream();
    let mut builder = Response::builder().status(status);
    if let Some(content_type) = content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    builder
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

impl ClaudeDesktopRuntimeState {
    pub(crate) async fn start(
        &self,
        credential: ToolCredential,
    ) -> Result<broadcast::Receiver<VerificationEvent>, AdapterFailure> {
        let local_token = credential
            .local_gateway_token
            .clone()
            .filter(|value| value.starts_with("ycg-") && value.len() == 68)
            .ok_or(AdapterFailure::SecureStorageUnavailable)?;
        self.stop().await;
        let listener = TcpListener::bind(PROXY_ADDRESS)
            .await
            .map_err(|_| AdapterFailure::LaunchFailed)?;
        let (events, receiver) = broadcast::channel(8);
        let proxy_state = Arc::new(ProxyState {
            credential,
            local_token,
            client: reqwest::Client::builder()
                .no_proxy()
                .build()
                .map_err(|_| AdapterFailure::LaunchFailed)?,
            events: events.clone(),
        });
        let router = Router::new()
            .route("/claude-desktop/{*path}", any(proxy_request))
            .with_state(proxy_state);
        let (shutdown, shutdown_rx) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
        });
        *self.runtime.lock().await = Some(ProxyRuntime {
            shutdown: Some(shutdown),
            task,
            events,
        });
        Ok(receiver)
    }

    pub(crate) async fn stop(&self) {
        let runtime = self.runtime.lock().await.take();
        if let Some(mut runtime) = runtime {
            if let Some(shutdown) = runtime.shutdown.take() {
                let _ = shutdown.send(());
            }
            runtime.task.abort();
        }
    }
}

pub(crate) async fn verify_and_launch(
    state: &ClaudeDesktopRuntimeState,
    installation: &ResolvedInstallation,
    credential: ToolCredential,
) -> Result<(), AdapterFailure> {
    let expected_model = credential.model_id.clone();
    let mut events = state.start(credential).await?;
    launch(&installation.path)?;
    let observed = timeout(Duration::from_secs(90), async {
        loop {
            match events.recv().await {
                Ok(event) if event.model == expected_model => return Ok(()),
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => return Err(()),
            }
        }
    })
    .await;
    match observed {
        Ok(Ok(())) => Ok(()),
        _ => Err(AdapterFailure::VerificationFailed(
            "waiting_for_desktop_request",
        )),
    }
}

pub(crate) async fn resume_if_configured(state: ClaudeDesktopRuntimeState) {
    let Ok(credential) = tool_credentials::load("claude_desktop") else {
        return;
    };
    let Some(local_token) = credential.local_gateway_token.as_deref() else {
        return;
    };
    let Some(home) = super::user_home() else {
        return;
    };
    let Ok((_, _, profile_path, _)) = current_paths(&home) else {
        return;
    };
    let profile = common::snapshot(&profile_path)
        .ok()
        .flatten()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let active = profile.as_ref().is_some_and(|value| {
        value["inferenceGatewayBaseUrl"].as_str() == Some(PROXY_BASE)
            && value["inferenceGatewayApiKey"].as_str() == Some(local_token)
    });
    if active {
        let _ = state.start(credential).await;
    }
}

#[cfg(target_os = "macos")]
fn launch(path: &Path) -> Result<(), AdapterFailure> {
    std::process::Command::new("/usr/bin/open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|_| AdapterFailure::LaunchFailed)
}

#[cfg(target_os = "windows")]
fn launch(path: &Path) -> Result<(), AdapterFailure> {
    std::process::Command::new(path)
        .spawn()
        .map(|_| ())
        .map_err(|_| AdapterFailure::LaunchFailed)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn launch(_path: &Path) -> Result<(), AdapterFailure> {
    Err(AdapterFailure::UnsupportedProfile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_uses_local_token_and_safe_route_not_upstream_key() {
        let bytes =
            profile_config("deepseek-v4-flash", &format!("ycg-{}", "a".repeat(64))).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["inferenceGatewayBaseUrl"], PROXY_BASE);
        assert_eq!(value["inferenceModels"][0]["name"], SAFE_ROUTE_MODEL);
        assert_eq!(
            value["inferenceModels"][0]["labelOverride"],
            "deepseek-v4-flash"
        );
        assert!(!String::from_utf8(bytes).unwrap().contains("sk-secret"));
    }

    #[test]
    fn meta_merge_keeps_other_profiles() {
        let before = br#"{"entries":[{"id":"other","name":"Other"}],"future":true}"#;
        let bytes = meta_config(Some(before)).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["future"], true);
        assert!(value["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["id"] == "other"));
        assert_eq!(value["appliedId"], PROFILE_ID);
    }
}
