use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use reqwest::Url;
use serde_json::{json, Value as JsonValue};
use serde_yaml::{Mapping, Value};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::Mutex,
    task::JoinHandle,
    time::timeout,
};

use crate::tool_adapters::{
    common::{self, ConfigFailure, FileTransaction},
    AdapterFailure, ResolvedInstallation,
};

const VERIFICATION_PROMPT: &str = "仅回复 YESCHOY_OK，不要调用工具。";
const STARTUP_OUTPUT_LIMIT: usize = 32 * 1024;

fn start_arguments() -> [&'static str; 6] {
    ["web", "--host", "127.0.0.1", "--port", "0", "--no-open"]
}

fn headless_arguments() -> [&'static str; 3] {
    ["--profile", "headless", VERIFICATION_PROMPT]
}

pub(crate) struct DshRuntime {
    child: Child,
    url: String,
    stdout_task: JoinHandle<()>,
    identity: [u8; 32],
}

impl Drop for DshRuntime {
    fn drop(&mut self) {
        // The child has kill_on_drop; the separate pipe reader must not detach
        // if shutdown is cancelled at its shared deadline.
        self.stdout_task.abort();
    }
}

#[derive(Default)]
pub(crate) struct DshRuntimeState {
    runtime: Mutex<Option<DshRuntime>>,
}

impl DshRuntimeState {
    pub(crate) async fn stop(&self) {
        if let Some(mut runtime) = self.runtime.lock().await.take() {
            let _ = runtime.child.kill().await;
            runtime.stdout_task.abort();
        }
    }

    async fn ensure_running(
        &self,
        installation: &ResolvedInstallation,
        key: &str,
        identity: [u8; 32],
    ) -> Result<String, AdapterFailure> {
        let mut slot = self.runtime.lock().await;
        if let Some(runtime) = slot.as_mut() {
            if runtime.identity == identity && matches!(runtime.child.try_wait(), Ok(None)) {
                return Ok(runtime.url.clone());
            }
        }
        if let Some(mut previous) = slot.take() {
            let _ = previous.child.kill().await;
            previous.stdout_task.abort();
        }
        let runtime = start_process(installation, key, identity).await?;
        let url = runtime.url.clone();
        *slot = Some(runtime);
        Ok(url)
    }
}

pub(crate) struct Prepared {
    transaction: FileTransaction,
    path: PathBuf,
    origin: String,
    model: String,
    model_ids: Vec<String>,
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

fn mapping(value: &mut Value) -> Result<&mut Mapping, ()> {
    value.as_mapping_mut().ok_or(())
}

fn child_mapping<'a>(parent: &'a mut Mapping, key: &str) -> Result<&'a mut Mapping, ()> {
    let key_value = Value::String(key.to_owned());
    if !parent.contains_key(&key_value) {
        parent.insert(key_value.clone(), Value::Mapping(Mapping::new()));
    }
    parent
        .get_mut(&key_value)
        .and_then(Value::as_mapping_mut)
        .ok_or(())
}

#[cfg(test)]
fn render(existing: Option<&[u8]>, origin: &str, model: &str) -> Result<Vec<u8>, ()> {
    render_catalog(existing, origin, model, &[model.to_owned()])
}

fn render_catalog(
    existing: Option<&[u8]>,
    origin: &str,
    model: &str,
    model_ids: &[String],
) -> Result<Vec<u8>, ()> {
    let mut root = match existing {
        Some(bytes) if !bytes.is_empty() => {
            serde_yaml::from_slice::<Value>(bytes).map_err(|_| ())?
        }
        _ => Value::Mapping(Mapping::new()),
    };
    let root_map = mapping(&mut root)?;
    let llm = child_mapping(root_map, "llm-pi-ai")?;
    let providers = child_mapping(llm, "providers")?;
    let provider = child_mapping(providers, "yeschoy")?;
    provider.insert(
        Value::String("displayName".into()),
        Value::String("野菜API".into()),
    );
    provider.insert(
        Value::String("apiKeyEnv".into()),
        Value::String("YESCHOY_DSH_API_KEY".into()),
    );
    provider.insert(
        Value::String("api".into()),
        Value::String("openai-completions".into()),
    );
    provider.insert(
        Value::String("baseURL".into()),
        Value::String(format!("{}/v1", origin.trim_end_matches('/'))),
    );
    provider.insert(
        Value::String("compat".into()),
        serde_yaml::to_value(json!({
            "supportsDeveloperRole": false,
            "maxTokensField": "max_tokens"
        }))
        .map_err(|_| ())?,
    );
    let previous_models = provider
        .get(Value::String("models".into()))
        .map(serde_json::to_value)
        .transpose()
        .map_err(|_| ())?;
    let models = crate::tool_model_profile::native_chat_catalog(
        previous_models.as_ref(),
        model_ids,
        crate::tool_model_profile::ModelConsumer::Dsh,
    );
    provider.insert(
        Value::String("models".into()),
        serde_yaml::to_value(models).map_err(|_| ())?,
    );
    let defaults = child_mapping(root_map, "agent-default-model")?;
    defaults.insert(
        Value::String("provider".into()),
        Value::String("yeschoy".into()),
    );
    defaults.insert(Value::String("model".into()), Value::String(model.into()));
    let mut bytes = serde_yaml::to_string(&root).map_err(|_| ())?.into_bytes();
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    Ok(bytes)
}

pub(crate) fn dsh_home(
    default_home: &Path,
    custom: Option<OsString>,
) -> Result<PathBuf, AdapterFailure> {
    let Some(custom) = custom.filter(|value| !value.is_empty()) else {
        return Ok(default_home.join(".dsh"));
    };
    let path = PathBuf::from(custom);
    let safe = path.is_absolute()
        && path.parent().is_some()
        && !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir));
    if safe {
        Ok(path)
    } else {
        Err(AdapterFailure::ExternalOverride)
    }
}

pub(crate) fn prepare(home: &Path, origin: &str, model: &str) -> Result<Prepared, AdapterFailure> {
    prepare_inner(home, origin, model, &[model.to_owned()])
}

pub(crate) fn prepare_catalog(
    home: &Path,
    _origin: &str,
    model: &str,
    model_ids: &[String],
) -> Result<Prepared, AdapterFailure> {
    crate::chat_gateway::validate_catalog(model, model_ids)?;
    prepare_inner(
        home,
        &crate::chat_gateway::base_url("dsh_web").unwrap(),
        model,
        model_ids,
    )
}

fn prepare_inner(
    home: &Path,
    origin: &str,
    model: &str,
    model_ids: &[String],
) -> Result<Prepared, AdapterFailure> {
    let path = dsh_home(home, std::env::var_os("DSH_HOME"))?.join("settings.yaml");
    let before = common::snapshot(&path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let after = render_catalog(before.as_deref(), origin, model, model_ids)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
    Ok(Prepared {
        transaction: FileTransaction::stage_with_snapshot(path.clone(), before, after)
            .map_err(config_error)?,
        path,
        origin: origin.to_owned(),
        model: model.to_owned(),
        model_ids: model_ids.to_vec(),
    })
}

impl Prepared {
    pub(crate) fn changes(&self) -> &[common::FileChange] {
        self.transaction.changes()
    }

    pub(crate) fn commit(&mut self) -> Result<(), AdapterFailure> {
        self.transaction.commit().map_err(config_error)?;
        self.validate_readback(true)
    }

    pub(crate) fn validate_existing(&self) -> Result<(), AdapterFailure> {
        self.validate_readback(false)
    }

    fn validate_readback(&self, strict_default: bool) -> Result<(), AdapterFailure> {
        let bytes = common::snapshot(&self.path)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?
            .ok_or(AdapterFailure::ConfigurationFailed(
                "configuration_readback_failed",
            ))?;
        let value: JsonValue = serde_yaml::from_slice(&bytes)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
        let provider = &value["llm-pi-ai"]["providers"]["yeschoy"];
        let correct = provider["apiKeyEnv"].as_str() == Some("YESCHOY_DSH_API_KEY")
            && provider["api"].as_str() == Some("openai-completions")
            && provider["baseURL"].as_str()
                == Some(format!("{}/v1", self.origin.trim_end_matches('/')).as_str())
            && crate::chat_gateway::catalog_matches(
                &provider["models"],
                Some("id"),
                &self.model_ids,
            )
            && provider["compat"]["supportsDeveloperRole"].as_bool() == Some(false)
            && provider["compat"]["maxTokensField"].as_str() == Some("max_tokens")
            && value["agent-default-model"]["provider"].as_str() == Some("yeschoy")
            && crate::chat_gateway::default_matches(
                value["agent-default-model"]["model"].as_str(),
                &self.model,
                &self.model_ids,
                strict_default,
            );
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

fn validated_loopback_url(line: &str) -> Option<String> {
    let raw = line
        .trim()
        .strip_prefix("dsh web: ")?
        .split_whitespace()
        .next()?;
    let url = Url::parse(raw).ok()?;
    let query = url.query_pairs().collect::<Vec<_>>();
    let valid_query = query.is_empty()
        || (query.len() == 1
            && query[0].0 == "token"
            && !query[0].1.is_empty()
            && query[0].1.len() <= 512
            && query[0]
                .1
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')));
    if url.scheme() != "http"
        || !matches!(
            url.host_str(),
            Some("127.0.0.1") | Some("localhost") | Some("::1")
        )
        || url.port().is_none()
        || url.username() != ""
        || url.password().is_some()
        || url.path() != "/"
        || !valid_query
        || url.fragment().is_some()
    {
        return None;
    }
    Some(if query.is_empty() {
        url.to_string().trim_end_matches('/').to_owned()
    } else {
        url.to_string()
    })
}

async fn verify_headless(
    installation: &ResolvedInstallation,
    key: &str,
) -> Result<(), AdapterFailure> {
    let working = common::temporary_working_directory("dsh-headless-verify")
        .map_err(|_| AdapterFailure::VerificationFailed("verification_workspace_failed"))?;
    let mut command = Command::new(&installation.path);
    command
        .args(headless_arguments())
        .env("YESCHOY_DSH_API_KEY", key)
        .current_dir(&working);
    let result = common::run_bounded(command, Duration::from_secs(120))
        .await
        .map_err(|error| AdapterFailure::VerificationFailed(error.reason_code()));
    let _ = std::fs::remove_dir_all(&working);
    let output = result?;
    if !output.success {
        return Err(AdapterFailure::VerificationFailed("tool_request_failed"));
    }
    // The headless runner writes the final assistant text to stdout and sends
    // reasoning/progress to stderr. Never accept the prompt echoed in logs.
    // github.com/deepseek-ai/deepseek-harness/packages/bundle/headless
    if output.stdout.len() <= STARTUP_OUTPUT_LIMIT && common::verification_reply(&output.stdout) {
        Ok(())
    } else {
        Err(AdapterFailure::VerificationFailed("tool_response_invalid"))
    }
}

async fn start_process(
    installation: &ResolvedInstallation,
    key: &str,
    identity: [u8; 32],
) -> Result<DshRuntime, AdapterFailure> {
    let mut command = Command::new(&installation.path);
    command
        .args(start_arguments())
        .env("YESCHOY_DSH_API_KEY", key)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|_| AdapterFailure::LaunchFailed)?;
    let stdout = child.stdout.take().ok_or(AdapterFailure::LaunchFailed)?;
    let mut reader = BufReader::new(stdout);
    let ready = timeout(Duration::from_secs(30), async {
        let mut observed = 0usize;
        loop {
            let mut line = String::new();
            let read = reader
                .read_line(&mut line)
                .await
                .map_err(|_| AdapterFailure::LaunchFailed)?;
            if read == 0 {
                return Err(AdapterFailure::LaunchFailed);
            }
            observed = observed.saturating_add(read);
            if observed > STARTUP_OUTPUT_LIMIT {
                return Err(AdapterFailure::LaunchFailed);
            }
            if let Some(url) = validated_loopback_url(&line) {
                return Ok(url);
            }
        }
    })
    .await
    .map_err(|_| AdapterFailure::LaunchFailed)?;
    let Ok(url) = ready else {
        let _ = child.kill().await;
        return Err(AdapterFailure::LaunchFailed);
    };
    // Keep draining stdout for the entire lifetime of the child. Dropping the
    // pipe after reading the startup URL can otherwise terminate DSH with a
    // broken pipe while its browser UI is still in use.
    let stdout_task = tokio::spawn(async move {
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    });
    Ok(DshRuntime {
        child,
        url,
        stdout_task,
        identity,
    })
}

pub(crate) async fn open_existing(
    state: &DshRuntimeState,
    installation: &ResolvedInstallation,
    key: &str,
) -> Result<(), AdapterFailure> {
    let home = super::user_home().ok_or(AdapterFailure::LaunchFailed)?;
    let path = dsh_home(&home, std::env::var_os("DSH_HOME"))?.join("settings.yaml");
    let bytes = common::snapshot(&path)
        .map_err(|_| AdapterFailure::LaunchFailed)?
        .ok_or(AdapterFailure::LaunchFailed)?;
    // Fingerprint only in native memory: a changed credential, installation,
    // home or settings cannot accidentally reuse a stale process.
    let identity = runtime_identity(&installation.path, &path, key, &bytes)?;
    let url = state.ensure_running(installation, key, identity).await?;
    open_browser(&url)
}

fn runtime_identity(
    installation: &Path,
    path: &Path,
    key: &str,
    bytes: &[u8],
) -> Result<[u8; 32], AdapterFailure> {
    // DSH reloads model settings on its next request. Catalog/default changes
    // do not change the process environment or require killing a live session.
    let mut settings: JsonValue =
        serde_yaml::from_slice(bytes).map_err(|_| AdapterFailure::LaunchFailed)?;
    if settings["llm-pi-ai"]["providers"]["yeschoy"]["baseURL"].as_str()
        == Some(format!("{}/v1", crate::chat_gateway::base_url("dsh_web").unwrap()).as_str())
    {
        if let Some(provider) = settings["llm-pi-ai"]["providers"]["yeschoy"].as_object_mut() {
            provider.remove("models");
        }
        if let Some(defaults) = settings["agent-default-model"].as_object_mut() {
            defaults.remove("model");
        }
    }
    let settings = serde_json::to_vec(&settings).map_err(|_| AdapterFailure::LaunchFailed)?;
    let mut fingerprint = Sha256::new();
    for part in [
        installation.to_string_lossy().as_bytes(),
        path.to_string_lossy().as_bytes(),
        key.as_bytes(),
        settings.as_slice(),
    ] {
        fingerprint.update((part.len() as u64).to_le_bytes());
        fingerprint.update(part);
    }
    Ok(fingerprint.finalize().into())
}

pub(crate) async fn verify_launch_and_keep(
    state: &DshRuntimeState,
    installation: &ResolvedInstallation,
    key: &str,
    _model: &str,
) -> Result<(), AdapterFailure> {
    verify_headless(installation, key).await?;
    open_existing(state, installation, key).await
}

#[cfg(target_os = "macos")]
fn open_browser(url: &str) -> Result<(), AdapterFailure> {
    std::process::Command::new("/usr/bin/open")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|_| AdapterFailure::LaunchFailed)
}

#[cfg(target_os = "windows")]
fn open_browser(url: &str) -> Result<(), AdapterFailure> {
    std::process::Command::new("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|_| AdapterFailure::LaunchFailed)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn open_browser(_url: &str) -> Result<(), AdapterFailure> {
    Err(AdapterFailure::UnsupportedProfile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ru043_dsh_catalog_efforts_preserve_existing_budget() {
        let existing = b"llm-pi-ai:\n  providers:\n    yeschoy:\n      reasoning: low\n      models:\n        - id: deepseek-v4-flash\n          maxTokens: 4096\n          contextWindow: 65536\n";
        let bytes = render_catalog(
            Some(existing),
            "http://127.0.0.1:15730/dsh_web",
            "deepseek-v4-flash",
            &["deepseek-v4-flash".into()],
        )
        .unwrap();
        let value: serde_json::Value = serde_yaml::from_slice(&bytes).unwrap();
        let provider = &value["llm-pi-ai"]["providers"]["yeschoy"];
        assert_eq!(provider["reasoning"], "low");
        let model = &provider["models"][0];
        assert_eq!(model["maxTokens"], 4096);
        assert_eq!(model["contextWindow"], 65536);
        assert_eq!(
            model["reasoningEfforts"],
            json!({"off":"none","low":"low","high":"high","max":"max"})
        );
        assert!(model.get("thinkingLevelMap").is_none());
    }

    #[test]
    fn ru042_dsh_catalog_default_change_preserves_runtime_identity() {
        let home = common::temporary_working_directory("dsh-model-set").unwrap();
        let path = home.join("settings.yaml");
        let installation = home.join("synthetic-dsh");
        let ids = vec!["model-a".into(), "model-b".into()];
        let origin = crate::chat_gateway::base_url("dsh_web").unwrap();
        let bytes = render_catalog(None, &origin, "model-a", &ids).unwrap();
        let before = runtime_identity(&installation, &path, "synthetic-local", &bytes).unwrap();
        let mut prepared = Prepared {
            transaction: FileTransaction::stage_with_snapshot(path.clone(), None, bytes.clone())
                .unwrap(),
            path: path.clone(),
            origin,
            model: "model-a".into(),
            model_ids: ids,
        };
        prepared.commit().unwrap();
        let mut settings: JsonValue = serde_yaml::from_slice(&bytes).unwrap();
        settings["agent-default-model"]["model"] = "model-b".into();
        let changed = serde_yaml::to_string(&settings).unwrap();
        std::fs::write(&path, &changed).unwrap();
        assert!(prepared.validate_existing().is_ok());
        assert!(prepared.validate_readback(true).is_err());
        assert_eq!(
            before,
            runtime_identity(&installation, &path, "synthetic-local", changed.as_bytes()).unwrap()
        );
        settings["llm-pi-ai"]["providers"]["yeschoy"]["models"] =
            json!([{"id":"model-c","name":"model-c","input":["text"]}]);
        let catalog_change = serde_yaml::to_string(&settings).unwrap();
        assert_eq!(
            before,
            runtime_identity(
                &installation,
                &path,
                "synthetic-local",
                catalog_change.as_bytes()
            )
            .unwrap()
        );
        assert_ne!(
            before,
            runtime_identity(
                &installation,
                &path,
                "rotated-local",
                catalog_change.as_bytes()
            )
            .unwrap()
        );
        settings["agent-default-model"]["model"] = "not-enrolled".into();
        std::fs::write(&path, serde_yaml::to_string(&settings).unwrap()).unwrap();
        assert!(prepared.validate_existing().is_err());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ru042_dsh_catalog_update_reuses_real_fixture_process() {
        let fixture = common::test_support::Script::new("[ \"$1\" = web ] || exit 11\n[ \"$YESCHOY_DSH_API_KEY\" = synthetic-local ] || exit 12\nprintf 'dsh web: http://127.0.0.1:3018/?token=synthetic\\n'\nexec /bin/sleep 30");
        let installation = fixture.installation();
        let config = installation.path.with_extension("yaml");
        let origin = crate::chat_gateway::base_url("dsh_web").unwrap();
        let a = render_catalog(None, &origin, "model-a", &["model-a".into()]).unwrap();
        let b = render_catalog(
            None,
            &origin,
            "model-b",
            &["model-a".into(), "model-b".into()],
        )
        .unwrap();
        let a = runtime_identity(&installation.path, &config, "synthetic-local", &a).unwrap();
        let b = runtime_identity(&installation.path, &config, "synthetic-local", &b).unwrap();
        let state = DshRuntimeState::default();
        state
            .ensure_running(&installation, "synthetic-local", a)
            .await
            .unwrap();
        let pid = state.runtime.lock().await.as_ref().unwrap().child.id();
        state
            .ensure_running(&installation, "synthetic-local", b)
            .await
            .unwrap();
        assert_eq!(state.runtime.lock().await.as_ref().unwrap().child.id(), pid);
        state.stop().await;
        assert!(state.runtime.lock().await.is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dsh_verification_executes_headless_fixture_and_rejects_prompt_echo() {
        use common::test_support::Script;
        let valid = Script::new("[ \"$1\" = --profile ] || exit 11\n[ \"$2\" = headless ] || exit 12\n[ \"$YESCHOY_DSH_API_KEY\" = synthetic-only ] || exit 13\nprintf 'dsh: reasoning: fixture\\n' >&2\nprintf 'YESCHOY_OK\\n'");
        assert!(verify_headless(&valid.installation(), "synthetic-only")
            .await
            .is_ok());
        for body in [
            "printf 'DSH ready\\n'",
            "printf 'Usage: dsh --profile headless\\n'",
            "printf '%s\\n' \"$*\"",
            "printf 'YESCHOY_OK\\n'; exit 17",
        ] {
            let script = Script::new(body);
            assert!(
                verify_headless(&script.installation(), "synthetic-only")
                    .await
                    .is_err(),
                "{body}"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn daily_open_reuses_live_dsh_and_restarts_exited_child_without_probe() {
        use std::os::unix::fs::PermissionsExt;
        let home = common::temporary_working_directory("dsh-open").unwrap();
        let path = home.join("fake-dsh");
        // Any headless/model probe fails: only the existing web startup
        // arguments may be used by daily Open.
        std::fs::write(&path, b"#!/bin/sh\n[ \"$#\" -eq 6 ] || exit 61\n[ \"$1\" = web ] || exit 62\n[ \"$6\" = --no-open ] || exit 63\nprintf 'dsh web: http://127.0.0.1:3018/?token=synthetic\\n'\nexec /bin/sleep 60\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let installation = ResolvedInstallation { path };
        let state = DshRuntimeState::default();
        let (a, b) = tokio::join!(
            state.ensure_running(&installation, "synthetic-key", [1; 32]),
            state.ensure_running(&installation, "synthetic-key", [1; 32]),
        );
        assert_eq!(a.unwrap(), b.unwrap());
        let first_pid = state
            .runtime
            .lock()
            .await
            .as_ref()
            .unwrap()
            .child
            .id()
            .unwrap();
        state
            .ensure_running(&installation, "synthetic-key", [1; 32])
            .await
            .unwrap();
        assert_eq!(
            state.runtime.lock().await.as_ref().unwrap().child.id(),
            Some(first_pid)
        );
        state
            .runtime
            .lock()
            .await
            .as_mut()
            .unwrap()
            .child
            .kill()
            .await
            .unwrap();
        state
            .ensure_running(&installation, "synthetic-key", [1; 32])
            .await
            .unwrap();
        let second_pid = state
            .runtime
            .lock()
            .await
            .as_ref()
            .unwrap()
            .child
            .id()
            .unwrap();
        assert_ne!(first_pid, second_pid);
        state
            .ensure_running(&installation, "synthetic-new-key", [2; 32])
            .await
            .unwrap();
        assert_ne!(
            state.runtime.lock().await.as_ref().unwrap().child.id(),
            Some(second_pid)
        );
        state.stop().await;
        assert!(state.runtime.lock().await.is_none());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn dsh_existing_validation_rejects_changed_destination_without_writing() {
        let home = common::temporary_working_directory("dsh-existing").unwrap();
        let path = home.join("settings.yaml");
        let bytes = render(None, "https://yeschoy.com", "test-model").unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let prepared = Prepared {
            transaction: FileTransaction::stage_with_snapshot(
                path.clone(),
                Some(bytes.clone()),
                bytes.clone(),
            )
            .unwrap(),
            path: path.clone(),
            origin: "https://yeschoy.com".into(),
            model: "test-model".into(),
            model_ids: vec!["test-model".into()],
        };
        assert!(prepared.validate_existing().is_ok());
        let changed = String::from_utf8(bytes)
            .unwrap()
            .replace("https://yeschoy.com/v1", "https://different.example/v1");
        std::fs::write(&path, &changed).unwrap();
        assert!(prepared.validate_existing().is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), changed);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn yaml_merge_preserves_other_routes_and_contains_only_a_reference() {
        let before = br#"ui-onboarding:
  welcomeNoticeVersion: keep
llm-pi-ai:
  providers:
    local:
      baseURL: http://127.0.0.1:11434/v1
"#;
        let bytes = render(Some(before), "https://yeschoy.com", "glm-5.3").unwrap();
        let value: JsonValue = serde_yaml::from_slice(&bytes).unwrap();
        assert_eq!(value["ui-onboarding"]["welcomeNoticeVersion"], "keep");
        assert_eq!(
            value["llm-pi-ai"]["providers"]["local"]["baseURL"],
            "http://127.0.0.1:11434/v1"
        );
        assert_eq!(
            value["llm-pi-ai"]["providers"]["yeschoy"]["apiKeyEnv"],
            "YESCHOY_DSH_API_KEY"
        );
        assert_eq!(
            value["llm-pi-ai"]["providers"]["yeschoy"]["compat"]["supportsDeveloperRole"],
            false
        );
        assert_eq!(
            value["llm-pi-ai"]["providers"]["yeschoy"]["compat"]["maxTokensField"],
            "max_tokens"
        );
        assert!(!String::from_utf8(bytes).unwrap().contains("sk-secret"));
    }

    #[test]
    fn only_loopback_startup_urls_are_accepted() {
        assert!(validated_loopback_url("dsh web: http://127.0.0.1:3018").is_some());
        assert!(
            validated_loopback_url("dsh web: http://127.0.0.1:3018/?token=abc_DEF-123").is_some()
        );
        assert!(validated_loopback_url("dsh web: https://evil.example:3018").is_none());
        assert!(validated_loopback_url("debug http://127.0.0.1:3018").is_none());
        assert!(validated_loopback_url("dsh web: http://127.0.0.1:3018/admin").is_none());
        assert!(validated_loopback_url("dsh web: http://127.0.0.1:3018/?next=evil").is_none());
        assert!(
            validated_loopback_url("dsh web: http://127.0.0.1:3018/?token=ok&next=evil").is_none()
        );
    }

    #[test]
    fn custom_dsh_home_is_used_when_safe() {
        let default = common::temporary_working_directory("dsh-default-home").unwrap();
        let custom = common::temporary_working_directory("dsh-custom-home").unwrap();
        assert_eq!(
            dsh_home(&default, Some(custom.clone().into_os_string())).unwrap(),
            custom
        );
        assert_eq!(dsh_home(&default, None).unwrap(), default.join(".dsh"));
        assert!(dsh_home(&default, Some(OsString::from("relative/dsh"))).is_err());
        let _ = std::fs::remove_dir_all(default);
        let _ = std::fs::remove_dir_all(custom);
    }

    #[test]
    fn latest_release_candidate_does_not_open_before_verification() {
        assert_eq!(
            start_arguments(),
            ["web", "--host", "127.0.0.1", "--port", "0", "--no-open"]
        );
        assert_eq!(
            headless_arguments(),
            ["--profile", "headless", VERIFICATION_PROMPT]
        );
    }
}
