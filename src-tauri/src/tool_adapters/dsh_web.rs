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
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, Command},
    sync::Mutex,
    task::JoinHandle,
    time::timeout,
};

use crate::tool_adapters::{
    common::{self, ConfigFailure, FileTransaction},
    AdapterFailure, ResolvedInstallation,
};

const STARTUP_OUTPUT_LIMIT: usize = 32 * 1024;

async fn read_bounded_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    maximum: usize,
) -> std::io::Result<Vec<u8>> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(line);
        }
        let take = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        if line.len().saturating_add(take) > maximum {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "DSH startup line exceeded the bounded output budget",
            ));
        }
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if line.last() == Some(&b'\n') {
            return Ok(line);
        }
    }
}

fn start_arguments() -> [&'static str; 7] {
    [
        "--profile",
        "web",
        "--host",
        "127.0.0.1",
        "--port",
        "0",
        "--no-open",
    ]
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

const PATCH_FILENAME: &str = "cordis.patch.yml";
const CREDENTIALS_FILENAME: &str = ".credentials.yaml";
const KEY_REFERENCE: &str = "YESCHOY_DSH_API_KEY";
const LLM_ENTRY: &str = "llm-pi-ai";
const LLM_PLUGIN: &str = "@deepseek-ai/dsh-llm-pi-ai";
const DEFAULT_MODEL_ENTRY: &str = "agent-default-model";
const DEFAULT_MODEL_PLUGIN: &str = "@deepseek-ai/dsh-agent-default-model";
/// A DSH writer holds its lock for one read-render-rename cycle. A lock older
/// than this was left by a process that died holding it.
const STALE_LOCK: Duration = Duration::from_secs(60);
const LOCK_WAIT: Duration = Duration::from_secs(2);

/// Which DSH profile a connection configures.
///
/// Both read the same `$DSH_HOME`; each owns its own patch. They differ in how
/// the key arrives: `dsh --profile web` is started by this app, which puts the
/// key in its environment, while DeepSeek Harness Desktop is started by the
/// user and can only read it from DSH's shared credentials file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DshProfile {
    Web,
    Desktop,
}

impl DshProfile {
    fn name(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Desktop => "desktop",
        }
    }
}

pub(crate) struct Prepared {
    transaction: FileTransaction,
    /// `$DSH_HOME/profiles/<profile>/cordis.patch.yml`, always written.
    patch_path: PathBuf,
    /// `$DSH_HOME/.credentials.yaml`, written for the Desktop profile only.
    credentials_path: Option<PathBuf>,
    /// `$DSH_HOME/settings.yaml`, written only when it already exists.
    settings_path: Option<PathBuf>,
    origin: String,
    model: String,
    model_ids: Vec<String>,
    key: Option<String>,
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

/// Our provider, over whatever the user already had under the same name.
///
/// Fields we do not own (`reasoning`, per-model budgets) survive, the same
/// rule `native_chat_catalog` applies to each model entry.
fn yeschoy_provider(
    previous: Option<&Value>,
    origin: &str,
    model_ids: &[String],
) -> Result<Value, ()> {
    let mut provider = match previous {
        Some(Value::Mapping(existing)) => existing.clone(),
        _ => Mapping::new(),
    };
    provider.insert(
        Value::String("displayName".into()),
        Value::String("野菜API".into()),
    );
    provider.insert(
        Value::String("apiKeyEnv".into()),
        Value::String(KEY_REFERENCE.into()),
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
    Ok(Value::Mapping(provider))
}

fn to_yaml_bytes(value: &Value) -> Result<Vec<u8>, ()> {
    let mut bytes = serde_yaml::to_string(value).map_err(|_| ())?.into_bytes();
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    Ok(bytes)
}

#[cfg(test)]
fn render(existing: Option<&[u8]>, origin: &str, model: &str) -> Result<Vec<u8>, ()> {
    render_catalog(existing, origin, model, &[model.to_owned()])
}

/// DSH 0.1's `settings.yaml`: the user layer, which wins over the profile patch.
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
    let llm = child_mapping(root_map, LLM_ENTRY)?;
    let providers = child_mapping(llm, "providers")?;
    let previous = providers.get(Value::String("yeschoy".into())).cloned();
    providers.insert(
        Value::String("yeschoy".into()),
        yeschoy_provider(previous.as_ref(), origin, model_ids)?,
    );
    let defaults = child_mapping(root_map, DEFAULT_MODEL_ENTRY)?;
    defaults.insert(
        Value::String("provider".into()),
        Value::String("yeschoy".into()),
    );
    defaults.insert(Value::String("model".into()), Value::String(model.into()));
    to_yaml_bytes(&root)
}

fn is_entry_row(row: &Value, id: &str) -> bool {
    // An `insert` row adds entries rather than configuring one.
    row.get("id").and_then(Value::as_str) == Some(id) && row.get("insert").is_none()
}

/// The row's `config` mapping, created when the row has none. Any other shape
/// is somebody else's intent and is refused rather than overwritten.
fn row_config(row: &mut Value) -> Result<&mut Mapping, ()> {
    let row = mapping(row)?;
    let key = Value::String("config".into());
    if matches!(row.get(&key), None | Some(Value::Null)) {
        row.insert(key.clone(), Value::Mapping(Mapping::new()));
    }
    row.get_mut(&key).and_then(Value::as_mapping_mut).ok_or(())
}

fn new_row(id: &str, plugin: &str) -> Value {
    let mut row = Mapping::new();
    row.insert(Value::String("id".into()), Value::String(id.into()));
    row.insert(Value::String("name".into()), Value::String(plugin.into()));
    row.insert(
        Value::String("config".into()),
        Value::Mapping(Mapping::new()),
    );
    Value::Mapping(row)
}

fn parse_patch(existing: Option<&[u8]>) -> Result<Vec<Value>, ()> {
    let text = match existing {
        None => return Ok(Vec::new()),
        Some(bytes) => std::str::from_utf8(bytes).map_err(|_| ())?,
    };
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    match serde_yaml::from_str::<Value>(text).map_err(|_| ())? {
        Value::Null => Ok(Vec::new()),
        Value::Sequence(rows) => Ok(rows),
        // DSH refuses anything but a sequence; so do we, without touching it.
        _ => Err(()),
    }
}

/// A profile's `cordis.patch.yml`, which both DSH 0.1 and 0.2 read.
///
/// A patch row replaces its entry's whole `config`, so the `llm-pi-ai` row is
/// merged into, never replaced: every other provider the user added on the
/// Models page stays. Every row configuring an entry is updated, because the
/// last one wins and a stale earlier one would otherwise be shadowed by ours
/// or the other way round.
fn render_profile_patch(
    existing: Option<&[u8]>,
    origin: &str,
    model: &str,
    model_ids: &[String],
) -> Result<Vec<u8>, ()> {
    let mut rows = parse_patch(existing)?;
    if !rows.iter().any(|row| is_entry_row(row, LLM_ENTRY)) {
        rows.push(new_row(LLM_ENTRY, LLM_PLUGIN));
    }
    if !rows
        .iter()
        .any(|row| is_entry_row(row, DEFAULT_MODEL_ENTRY))
    {
        rows.push(new_row(DEFAULT_MODEL_ENTRY, DEFAULT_MODEL_PLUGIN));
    }
    for row in &mut rows {
        if is_entry_row(row, LLM_ENTRY) {
            let providers = child_mapping(row_config(row)?, "providers")?;
            let previous = providers.get(Value::String("yeschoy".into())).cloned();
            providers.insert(
                Value::String("yeschoy".into()),
                yeschoy_provider(previous.as_ref(), origin, model_ids)?,
            );
        } else if is_entry_row(row, DEFAULT_MODEL_ENTRY) {
            let config = row_config(row)?;
            let unchanged = config.get("provider").and_then(Value::as_str) == Some("yeschoy")
                && config.get("model").and_then(Value::as_str) == Some(model);
            if !unchanged {
                // An effort chosen for some other model need not exist on ours.
                config.remove("reasoningEffort");
            }
            config.insert(
                Value::String("provider".into()),
                Value::String("yeschoy".into()),
            );
            config.insert(Value::String("model".into()), Value::String(model.into()));
        }
    }
    to_yaml_bytes(&Value::Sequence(rows))
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

/// The files this adapter may write, in the order they are staged.
///
/// `settings.yaml` is listed only while it exists. DSH 0.1 layers it over the
/// profile patch, so a value left there would win over ours; 0.2 imports it
/// into whichever profile boots next and renames it, so creating one would
/// write into a profile we do not own.
pub(crate) fn configuration_paths(
    home: &Path,
    profile: DshProfile,
) -> Result<Vec<PathBuf>, AdapterFailure> {
    // `DSH_HOME` is exported from a shell rc, which a Dock-launched app never
    // sources. Reading it from this process wrote settings.yaml into ~/.dsh
    // while `dsh` itself loaded a different directory.
    let root = dsh_home(home, crate::shell_environment::var_os("DSH_HOME"))?;
    Ok(paths_in(&root, profile))
}

fn paths_in(root: &Path, profile: DshProfile) -> Vec<PathBuf> {
    let mut paths = vec![root
        .join("profiles")
        .join(profile.name())
        .join(PATCH_FILENAME)];
    if profile == DshProfile::Desktop {
        paths.push(root.join(CREDENTIALS_FILENAME));
    }
    let settings = root.join("settings.yaml");
    if settings.is_file() {
        paths.push(settings);
    }
    paths
}

pub(crate) fn prepare(home: &Path, origin: &str, model: &str) -> Result<Prepared, AdapterFailure> {
    let root = dsh_home(home, crate::shell_environment::var_os("DSH_HOME"))?;
    prepare_in(
        &root,
        DshProfile::Web,
        origin,
        model,
        &[model.to_owned()],
        None,
    )
}

pub(crate) fn prepare_catalog(
    home: &Path,
    origin: &str,
    model: &str,
    model_ids: &[String],
) -> Result<Prepared, AdapterFailure> {
    crate::tool_adapters::common::validate_catalog(model, model_ids)?;
    let root = dsh_home(home, crate::shell_environment::var_os("DSH_HOME"))?;
    prepare_in(&root, DshProfile::Web, origin, model, model_ids, None)
}

/// DeepSeek Harness Desktop: its own profile, and the key in DSH's credentials
/// file, because nothing this app starts stands between Desktop and the relay.
pub(crate) fn prepare_desktop(
    home: &Path,
    origin: &str,
    model: &str,
    key: &str,
) -> Result<Prepared, AdapterFailure> {
    let root = dsh_home(home, crate::shell_environment::var_os("DSH_HOME"))?;
    prepare_in(
        &root,
        DshProfile::Desktop,
        origin,
        model,
        &[model.to_owned()],
        Some(key),
    )
}

pub(crate) fn prepare_desktop_catalog(
    home: &Path,
    origin: &str,
    model: &str,
    model_ids: &[String],
    key: &str,
) -> Result<Prepared, AdapterFailure> {
    crate::tool_adapters::common::validate_catalog(model, model_ids)?;
    let root = dsh_home(home, crate::shell_environment::var_os("DSH_HOME"))?;
    prepare_in(
        &root,
        DshProfile::Desktop,
        origin,
        model,
        model_ids,
        Some(key),
    )
}

fn prepare_in(
    root: &Path,
    profile: DshProfile,
    origin: &str,
    model: &str,
    model_ids: &[String],
    key: Option<&str>,
) -> Result<Prepared, AdapterFailure> {
    let parse_failed = AdapterFailure::ConfigurationFailed("configuration_parse_failed");
    if (profile == DshProfile::Desktop) != key.is_some() {
        return Err(AdapterFailure::ConfigurationFailed("invalid_request"));
    }
    let read = |path: &Path| {
        common::snapshot(path)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))
    };
    let mut transaction = FileTransaction::default();
    let (mut patch_path, mut credentials_path, mut settings_path) = (None, None, None);
    for path in paths_in(root, profile) {
        let before = read(&path)?;
        let after = match path.file_name().and_then(|name| name.to_str()) {
            Some(PATCH_FILENAME) => {
                patch_path = Some(path.clone());
                render_profile_patch(before.as_deref(), origin, model, model_ids)
            }
            Some(CREDENTIALS_FILENAME) => {
                credentials_path = Some(path.clone());
                render_credentials(before.as_deref(), key.unwrap_or_default())
            }
            _ => {
                settings_path = Some(path.clone());
                render_catalog(before.as_deref(), origin, model, model_ids)
            }
        }
        .map_err(|_| parse_failed)?;
        transaction
            .push_with_snapshot(path, before, after)
            .map_err(config_error)?;
    }
    Ok(Prepared {
        transaction,
        patch_path: patch_path.ok_or(parse_failed)?,
        credentials_path,
        settings_path,
        origin: origin.to_owned(),
        model: model.to_owned(),
        model_ids: model_ids.to_vec(),
        key: key.map(str::to_owned),
    })
}

/// DSH's credentials document: `version: 1` and a `refs` map of reference name
/// to secret. Only our reference is written; every other ref, and any
/// `records`, stay as they are.
///
/// A non-empty document without `version` is the pre-release flat layout,
/// which DSH itself refuses to start with and asks the user to migrate. It is
/// refused here as well rather than rewritten on their behalf.
fn render_credentials(existing: Option<&[u8]>, key: &str) -> Result<Vec<u8>, ()> {
    if key.is_empty() {
        return Err(());
    }
    let mut root = match existing {
        Some(bytes) if !String::from_utf8_lossy(bytes).trim().is_empty() => {
            match serde_yaml::from_slice::<Value>(bytes).map_err(|_| ())? {
                Value::Null => Mapping::new(),
                Value::Mapping(map) => map,
                _ => return Err(()),
            }
        }
        _ => Mapping::new(),
    };
    let version = Value::String("version".into());
    match root.get(&version) {
        None if root.is_empty() => {
            root.insert(version, Value::Number(1.into()));
        }
        Some(Value::Number(number)) if number.as_u64() == Some(1) => {}
        _ => return Err(()),
    }
    let refs = child_mapping(&mut root, "refs")?;
    refs.insert(
        Value::String(KEY_REFERENCE.into()),
        Value::String(key.into()),
    );
    to_yaml_bytes(&Value::Mapping(root))
}

/// DSH's cross-process writer locks: an exclusively created `<file>.lock`
/// holding the writer's PID, removed when the write is done. Its config editor
/// takes `<profile>/package.json.lock` around every patch write, and its
/// credentials store `.credentials.yaml.lock` around every key change.
struct ProfileLock(Vec<PathBuf>);

impl ProfileLock {
    fn acquire(prepared: &Prepared) -> Result<Self, AdapterFailure> {
        let busy = AdapterFailure::ConfigurationFailed("configuration_write_failed");
        let mut targets = vec![prepared
            .patch_path
            .parent()
            .ok_or(busy)?
            .join("package.json")];
        targets.extend(prepared.credentials_path.clone());
        let mut held = Self(Vec::new());
        for target in targets {
            // A directory that does not exist yet has no DSH writing into it.
            if target.parent().is_some_and(Path::is_dir) {
                held.0.push(Self::take(&target)?);
            }
        }
        Ok(held)
    }

    fn take(target: &Path) -> Result<PathBuf, AdapterFailure> {
        let busy = AdapterFailure::ConfigurationFailed("configuration_write_failed");
        let mut lock = target.as_os_str().to_owned();
        lock.push(".lock");
        let lock = PathBuf::from(lock);
        let deadline = std::time::Instant::now() + LOCK_WAIT;
        let mut stale_removed = false;
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock)
            {
                Ok(mut file) => {
                    use std::io::Write;
                    let _ = writeln!(file, "{}", std::process::id());
                    return Ok(lock);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(busy),
            }
            let stale = std::fs::metadata(&lock)
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age > STALE_LOCK);
            if stale && !stale_removed {
                stale_removed = true;
                let _ = std::fs::remove_file(&lock);
                continue;
            }
            if std::time::Instant::now() >= deadline {
                return Err(busy);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for ProfileLock {
    fn drop(&mut self) {
        for lock in self.0.drain(..).rev() {
            let _ = std::fs::remove_file(lock);
        }
    }
}

/// The last row configuring `id`, which is the one DSH applies.
fn last_row_config<'a>(rows: &'a JsonValue, id: &str) -> Option<&'a JsonValue> {
    rows.as_array()?
        .iter()
        .rev()
        .find(|row| row["id"].as_str() == Some(id) && row.get("insert").is_none())
        .map(|row| &row["config"])
}

impl Prepared {
    pub(crate) fn changes(&self) -> &[common::FileChange] {
        self.transaction.changes()
    }

    pub(crate) fn commit(&mut self) -> Result<(), AdapterFailure> {
        let lock = ProfileLock::acquire(self)?;
        self.transaction.commit().map_err(config_error)?;
        drop(lock);
        // DSH refuses to start while the credentials file is readable by
        // anyone else. A new file is already 0600; an older, wider one is not
        // ours to leave that way once it holds our key.
        if let Some(path) = &self.credentials_path {
            common::narrow_third_party_file(path)
                .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_write_failed"))?;
        }
        self.validate_readback(true)
    }

    pub(crate) fn validate_existing(&self) -> Result<(), AdapterFailure> {
        self.validate_readback(false)
    }

    fn provider_matches(&self, provider: &JsonValue) -> bool {
        provider["apiKeyEnv"].as_str() == Some(KEY_REFERENCE)
            && provider["api"].as_str() == Some("openai-completions")
            && provider["baseURL"].as_str()
                == Some(format!("{}/v1", self.origin.trim_end_matches('/')).as_str())
            && crate::tool_adapters::common::catalog_matches(
                &provider["models"],
                Some("id"),
                &self.model_ids,
            )
            && provider["compat"]["supportsDeveloperRole"].as_bool() == Some(false)
            && provider["compat"]["maxTokensField"].as_str() == Some("max_tokens")
    }

    fn default_matches(&self, defaults: &JsonValue, strict_default: bool) -> bool {
        defaults["provider"].as_str() == Some("yeschoy")
            && crate::tool_adapters::common::default_matches(
                defaults["model"].as_str(),
                &self.model,
                &self.model_ids,
                strict_default,
            )
    }

    fn read_yaml(path: &Path) -> Result<JsonValue, AdapterFailure> {
        let failed = AdapterFailure::ConfigurationFailed("configuration_readback_failed");
        let bytes = common::snapshot(path).map_err(|_| failed)?.ok_or(failed)?;
        serde_yaml::from_slice(&bytes).map_err(|_| failed)
    }

    fn validate_readback(&self, strict_default: bool) -> Result<(), AdapterFailure> {
        let failed = AdapterFailure::ConfigurationFailed("configuration_readback_failed");
        let rows = Self::read_yaml(&self.patch_path)?;
        let patch_correct = last_row_config(&rows, LLM_ENTRY)
            .is_some_and(|config| self.provider_matches(&config["providers"]["yeschoy"]))
            && last_row_config(&rows, DEFAULT_MODEL_ENTRY)
                .is_some_and(|config| self.default_matches(config, strict_default));
        if !patch_correct {
            return Err(failed);
        }
        if let (Some(path), Some(key)) = (&self.credentials_path, &self.key) {
            let document = Self::read_yaml(path)?;
            if document["version"].as_u64() != Some(1)
                || document["refs"][KEY_REFERENCE].as_str() != Some(key.as_str())
            {
                return Err(failed);
            }
        }
        // DSH 0.2 renames `settings.yaml` to `settings.yaml.imported` once it
        // has moved it into the profile. Gone is the expected end state, not
        // a changed configuration; anything still there must agree with us,
        // because on 0.1 it wins.
        if let Some(path) = &self.settings_path {
            if path.is_file() {
                let value = Self::read_yaml(path)?;
                let settings_correct = self
                    .provider_matches(&value[LLM_ENTRY]["providers"]["yeschoy"])
                    && self.default_matches(&value[DEFAULT_MODEL_ENTRY], strict_default);
                if !settings_correct {
                    return Err(failed);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn rollback(&mut self) -> Result<(), AdapterFailure> {
        let lock = ProfileLock::acquire(self)?;
        let result = self.transaction.rollback().map_err(config_error);
        drop(lock);
        result
    }
}

/// Undo our rows in a profile patch, leaving everything else as it is now.
///
/// The generic three-way merge in `connection_recovery` treats a row that did
/// not exist before as wholly ours and drops it if it still contains what we
/// wrote. For `llm-pi-ai` that would take the providers the user added on the
/// Models page after connecting with it. Here only the values we own go back:
/// `providers.yeschoy` and the default model, each restored to what it was
/// (or removed) when it is still exactly what we wrote, and preserved when the
/// user has changed it since.
///
/// Returns `None` when the path is not a DSH profile patch. The flag reports
/// whether a value of ours was left in place because the user changed it.
pub(crate) fn restore_profile_patch(
    path: &Path,
    before: Option<&[u8]>,
    after: &[u8],
    current: &[u8],
) -> Option<Result<(Vec<JsonValue>, bool), ()>> {
    if path.file_name().and_then(|name| name.to_str()) != Some(PATCH_FILENAME) {
        return None;
    }
    let parse = |bytes: Option<&[u8]>| -> Result<Vec<JsonValue>, ()> {
        parse_patch(bytes)?
            .into_iter()
            .map(|row| serde_json::to_value(row).map_err(|_| ()))
            .collect()
    };
    Some((|| {
        let before = parse(before)?;
        let after = parse(Some(after))?;
        let mut now = parse(Some(current))?;
        let owned = |rows: &[JsonValue], id: &str, field: &[&str]| -> Option<JsonValue> {
            let config = rows
                .iter()
                .rev()
                .find(|row| row["id"].as_str() == Some(id) && row.get("insert").is_none())
                .map(|row| &row["config"])?;
            let mut value = config;
            for key in field {
                value = value.get(*key)?;
            }
            Some(value.clone())
        };
        let mut preserved = false;
        let provider = ["providers", "yeschoy"];
        let ours = owned(&after, LLM_ENTRY, &provider);
        let theirs = owned(&before, LLM_ENTRY, &provider);
        let default_ours = owned(&after, DEFAULT_MODEL_ENTRY, &[]);
        let default_theirs = owned(&before, DEFAULT_MODEL_ENTRY, &[]);
        let had_llm_row = before.iter().any(|row| row["id"] == LLM_ENTRY);
        let had_default_row = before.iter().any(|row| row["id"] == DEFAULT_MODEL_ENTRY);
        let mut index = 0;
        while index < now.len() {
            let row = &mut now[index];
            let entry = if row.get("insert").is_none() {
                row["id"].as_str().map(str::to_owned)
            } else {
                None
            };
            if entry.as_deref() == Some(LLM_ENTRY) {
                let providers = row
                    .get_mut("config")
                    .and_then(|config| config.get_mut("providers"))
                    .and_then(JsonValue::as_object_mut);
                if let Some(providers) = providers {
                    match (providers.get("yeschoy"), &ours) {
                        (Some(current), Some(ours)) if current == ours => match &theirs {
                            Some(value) => {
                                providers.insert("yeschoy".into(), value.clone());
                            }
                            None => {
                                providers.remove("yeschoy");
                            }
                        },
                        (Some(_), _) => preserved = true,
                        (None, _) => {}
                    }
                    if providers.is_empty() && !had_llm_row {
                        if let Some(config) = row["config"].as_object_mut() {
                            config.remove("providers");
                        }
                    }
                }
                let empty_config = row["config"]
                    .as_object()
                    .is_none_or(|config| config.is_empty());
                if empty_config && !had_llm_row {
                    now.remove(index);
                    continue;
                }
            } else if entry.as_deref() == Some(DEFAULT_MODEL_ENTRY) {
                if default_ours.as_ref() == Some(&row["config"]) {
                    match &default_theirs {
                        Some(value) => row["config"] = value.clone(),
                        None if !had_default_row => {
                            now.remove(index);
                            continue;
                        }
                        None => {
                            if let Some(row) = row.as_object_mut() {
                                row.remove("config");
                            }
                        }
                    }
                } else if row["config"]["provider"] == "yeschoy" {
                    preserved = true;
                }
            }
            index += 1;
        }
        Ok((now, preserved))
    })())
}

/// Undo our key in DSH's credentials file, leaving every other ref alone.
///
/// The generic merge would treat `version` as ours when the file did not exist
/// before, and drop it while a ref the user added since is still there, which
/// DSH then refuses as the old flat layout. The whole file goes only when
/// nothing but our own entries remains in a file we created.
///
/// Returns `None` when the path is not DSH's credentials file; otherwise the
/// document to write back (`None` to delete it) and whether our key was left
/// because it no longer holds the value we wrote.
pub(crate) fn restore_credentials(
    path: &Path,
    before: Option<&[u8]>,
    after: &[u8],
    current: &[u8],
) -> Option<Result<(Option<JsonValue>, bool), ()>> {
    if path.file_name().and_then(|name| name.to_str()) != Some(CREDENTIALS_FILENAME) {
        return None;
    }
    let parse = |bytes: &[u8]| -> Result<JsonValue, ()> {
        if String::from_utf8_lossy(bytes).trim().is_empty() {
            return Ok(JsonValue::Object(serde_json::Map::new()));
        }
        match serde_yaml::from_slice::<JsonValue>(bytes).map_err(|_| ())? {
            JsonValue::Null => Ok(JsonValue::Object(serde_json::Map::new())),
            value @ JsonValue::Object(_) => Ok(value),
            _ => Err(()),
        }
    };
    Some((|| {
        let theirs = before
            .map(parse)
            .transpose()?
            .and_then(|value| value["refs"].get(KEY_REFERENCE).cloned());
        let ours = parse(after)?["refs"].get(KEY_REFERENCE).cloned();
        let mut now = parse(current)?;
        let mut preserved = false;
        if let Some(refs) = now.get_mut("refs").and_then(JsonValue::as_object_mut) {
            match (refs.get(KEY_REFERENCE), &ours) {
                (Some(current), Some(ours)) if current == ours => match &theirs {
                    Some(value) => {
                        refs.insert(KEY_REFERENCE.into(), value.clone());
                    }
                    None => {
                        refs.remove(KEY_REFERENCE);
                    }
                },
                (Some(_), _) => preserved = true,
                (None, _) => {}
            }
            if refs.is_empty() {
                if let Some(document) = now.as_object_mut() {
                    document.remove("refs");
                }
            }
        }
        let only_version = now
            .as_object()
            .is_some_and(|document| document.keys().all(|key| key == "version"));
        if before.is_none() && only_version {
            return Ok((None, preserved));
        }
        Ok((Some(now), preserved))
    })())
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

async fn start_process(
    installation: &ResolvedInstallation,
    key: &str,
    identity: [u8; 32],
) -> Result<DshRuntime, AdapterFailure> {
    let mut command = Command::new(&installation.path);
    command
        .args(start_arguments())
        .env(KEY_REFERENCE, key)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    common::apply_cli_runtime_path(&mut command, &installation.path);
    let mut child = command.spawn().map_err(|_| AdapterFailure::LaunchFailed)?;
    let stdout = child.stdout.take().ok_or(AdapterFailure::LaunchFailed)?;
    let mut reader = BufReader::new(stdout);
    let ready = timeout(Duration::from_secs(15), async {
        let mut observed = 0usize;
        loop {
            let remaining = STARTUP_OUTPUT_LIMIT.saturating_sub(observed);
            let line = read_bounded_line(&mut reader, remaining)
                .await
                .map_err(|_| AdapterFailure::LaunchFailed)?;
            if line.is_empty() {
                return Err(AdapterFailure::LaunchFailed);
            }
            observed = observed.saturating_add(line.len());
            let line = std::str::from_utf8(&line).map_err(|_| AdapterFailure::LaunchFailed)?;
            if let Some(url) = validated_loopback_url(line) {
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
        let mut buffer = [0_u8; 8 * 1024];
        loop {
            match reader.read(&mut buffer).await {
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
    // Only the location: whether the settings in it are right was decided by
    // validation before Open was offered. Requiring `settings.yaml` here is
    // what made every Open fail once DSH 0.2 had imported and renamed it.
    let patch = configuration_paths(&home, DshProfile::Web)
        .map_err(|_| AdapterFailure::LaunchFailed)?
        .into_iter()
        .next()
        .ok_or(AdapterFailure::LaunchFailed)?;
    // Fingerprint only in native memory: a changed credential, installation
    // or home cannot accidentally reuse a stale process.
    let identity = runtime_identity(&installation.path, &patch, key);
    let url = state.ensure_running(installation, key, identity).await?;
    open_browser(&url)
}

/// What a running DSH cannot pick up without a restart.
///
/// Both 0.1 and 0.2 watch the profile patch and reload model settings on the
/// next request, so a catalog, default model or endpoint change does not end a
/// live session. The key is different: it reaches DSH through the process
/// environment, which only a new process gets.
fn runtime_identity(installation: &Path, patch: &Path, key: &str) -> [u8; 32] {
    let mut fingerprint = Sha256::new();
    for part in [
        installation.to_string_lossy().as_bytes(),
        patch.to_string_lossy().as_bytes(),
        key.as_bytes(),
    ] {
        fingerprint.update((part.len() as u64).to_le_bytes());
        fingerprint.update(part);
    }
    fingerprint.finalize().into()
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
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::System::SystemInformation::GetSystemDirectoryW;

    let required = unsafe { GetSystemDirectoryW(None) } as usize;
    if required == 0 {
        return Err(AdapterFailure::LaunchFailed);
    }
    let mut system_directory = vec![0_u16; required.saturating_add(1)];
    let written = unsafe { GetSystemDirectoryW(Some(&mut system_directory)) } as usize;
    if written == 0 || written >= system_directory.len() {
        return Err(AdapterFailure::LaunchFailed);
    }
    let rundll32 =
        PathBuf::from(OsString::from_wide(&system_directory[..written])).join("rundll32.exe");
    std::process::Command::new(rundll32)
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

    fn patch_rows(path: &Path) -> JsonValue {
        serde_yaml::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn ru042_dsh_catalog_default_change_preserves_runtime_identity() {
        let root = common::temporary_working_directory("dsh-model-set").unwrap();
        let ids = vec!["model-a".into(), "model-b".into()];
        let mut prepared = prepare_in(
            &root,
            DshProfile::Web,
            "https://yeschoy.com",
            "model-a",
            &ids,
            None,
        )
        .unwrap();
        prepared.commit().unwrap();
        let path = prepared.patch_path.clone();
        let installation = root.join("synthetic-dsh");
        let before = runtime_identity(&installation, &path, "synthetic-local");

        let mut rows = patch_rows(&path);
        let default = rows
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["id"] == DEFAULT_MODEL_ENTRY)
            .unwrap();
        default["config"]["model"] = "model-b".into();
        std::fs::write(&path, serde_yaml::to_string(&rows).unwrap()).unwrap();
        // A default the user picked in DSH's own picker, among ours, is fine;
        // it only fails the strict read-back right after our own write.
        assert!(prepared.validate_existing().is_ok());
        assert!(prepared.validate_readback(true).is_err());
        // Catalog and default changes are reloaded live, the key is not.
        assert_eq!(
            before,
            runtime_identity(&installation, &path, "synthetic-local")
        );
        assert_ne!(
            before,
            runtime_identity(&installation, &path, "rotated-local")
        );

        let default = rows
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["id"] == DEFAULT_MODEL_ENTRY)
            .unwrap();
        default["config"]["model"] = "not-enrolled".into();
        std::fs::write(&path, serde_yaml::to_string(&rows).unwrap()).unwrap();
        assert!(prepared.validate_existing().is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ru042_dsh_catalog_update_reuses_real_fixture_process() {
        let fixture = common::test_support::Script::new("[ \"$1\" = --profile ] || exit 11\n[ \"$2\" = web ] || exit 12\n[ \"$YESCHOY_DSH_API_KEY\" = synthetic-local ] || exit 13\nprintf 'dsh web: http://127.0.0.1:3018/?token=synthetic\\n'\nexec /bin/sleep 30");
        let installation = fixture.installation();
        let config = installation.path.with_extension("yml");
        // Reconnecting with another catalog keeps the same key and patch.
        let a = runtime_identity(&installation.path, &config, "synthetic-local");
        let b = runtime_identity(&installation.path, &config, "synthetic-local");
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
    async fn daily_open_reuses_live_dsh_and_restarts_exited_child_without_probe() {
        use std::os::unix::fs::PermissionsExt;
        let home = common::temporary_working_directory("dsh-open").unwrap();
        let path = home.join("fake-dsh");
        // No headless/model probe is allowed: only the existing web startup
        // arguments may be used by daily Open.
        std::fs::write(&path, b"#!/bin/sh\n[ \"$#\" -eq 7 ] || exit 61\n[ \"$1\" = --profile ] || exit 62\n[ \"$2\" = web ] || exit 63\n[ \"$7\" = --no-open ] || exit 64\nprintf 'dsh web: http://127.0.0.1:3018/?token=synthetic\\n'\nexec /bin/sleep 60\n").unwrap();
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
        let root = common::temporary_working_directory("dsh-existing").unwrap();
        let mut prepared = prepare_in(
            &root,
            DshProfile::Web,
            "https://yeschoy.com",
            "test-model",
            &["test-model".into()],
            None,
        )
        .unwrap();
        prepared.commit().unwrap();
        assert!(prepared.validate_existing().is_ok());
        let path = prepared.patch_path.clone();
        let changed = std::fs::read_to_string(&path)
            .unwrap()
            .replace("https://yeschoy.com/v1", "https://different.example/v1");
        std::fs::write(&path, &changed).unwrap();
        assert!(prepared.validate_existing().is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), changed);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn profile_patch_keeps_other_rows_and_providers_and_updates_every_row() {
        let before = br#"- id: llm-pi-ai
  name: '@deepseek-ai/dsh-llm-pi-ai'
  config:
    providers:
      local:
        baseURL: http://127.0.0.1:11434/v1
      yeschoy:
        reasoning: low
- id: agent-default-model
  config:
    provider: deepseek-official
    model: deepseek-flash
    reasoningEffort: max
- id: ui-settings
  config:
    theme: dark
- id: agent-default-model
  config:
    provider: deepseek-official
    model: deepseek-pro
"#;
        let bytes = render_profile_patch(
            Some(before),
            "https://yeschoy.com",
            "glm-5.3",
            &["glm-5.3".into()],
        )
        .unwrap();
        let rows: JsonValue = serde_yaml::from_slice(&bytes).unwrap();
        let rows = rows.as_array().unwrap();
        assert_eq!(rows.len(), 4, "no row is added when one exists");
        let llm = &rows[0]["config"]["providers"];
        assert_eq!(llm["local"]["baseURL"], "http://127.0.0.1:11434/v1");
        assert_eq!(llm["yeschoy"]["reasoning"], "low");
        assert_eq!(llm["yeschoy"]["baseURL"], "https://yeschoy.com/v1");
        assert_eq!(llm["yeschoy"]["apiKeyEnv"], "YESCHOY_DSH_API_KEY");
        assert_eq!(rows[2]["config"]["theme"], "dark");
        for index in [1, 3] {
            assert_eq!(rows[index]["config"]["provider"], "yeschoy");
            assert_eq!(rows[index]["config"]["model"], "glm-5.3");
            assert!(rows[index]["config"].get("reasoningEffort").is_none());
        }
        assert!(!String::from_utf8(bytes).unwrap().contains("sk-"));
    }

    #[test]
    fn profile_patch_is_created_with_named_rows_and_other_shapes_are_refused() {
        let bytes = render_profile_patch(None, "https://yeschoy.com", "m", &["m".into()]).unwrap();
        let rows: JsonValue = serde_yaml::from_slice(&bytes).unwrap();
        assert_eq!(rows[0]["id"], LLM_ENTRY);
        assert_eq!(rows[0]["name"], LLM_PLUGIN);
        assert_eq!(rows[1]["id"], DEFAULT_MODEL_ENTRY);
        assert_eq!(rows[1]["name"], DEFAULT_MODEL_PLUGIN);
        for empty in [&b""[..], b"   \n", b"~\n", b"[]\n"] {
            assert!(
                render_profile_patch(Some(empty), "https://yeschoy.com", "m", &["m".into()])
                    .is_ok()
            );
        }
        for foreign in [
            &b"llm-pi-ai: {}\n"[..],
            b"- id: llm-pi-ai\n  config: [1]\n",
            b"- [\n",
        ] {
            assert!(
                render_profile_patch(Some(foreign), "https://yeschoy.com", "m", &["m".into()])
                    .is_err()
            );
        }
        // An `insert` row adds entries; it is not the entry's configuration.
        let insert = b"- id: llm-pi-ai\n  insert: [{name: x}]\n";
        let bytes =
            render_profile_patch(Some(insert), "https://yeschoy.com", "m", &["m".into()]).unwrap();
        let rows: JsonValue = serde_yaml::from_slice(&bytes).unwrap();
        assert!(rows[0].get("config").is_none());
        assert_eq!(rows[1]["id"], LLM_ENTRY);
    }

    #[test]
    fn settings_yaml_is_written_only_while_it_exists_and_may_disappear() {
        let root = common::temporary_working_directory("dsh-settings-layer").unwrap();
        let ids = vec!["m".to_string()];
        let prepared = prepare_in(
            &root,
            DshProfile::Web,
            "https://yeschoy.com",
            "m",
            &ids,
            None,
        )
        .unwrap();
        assert_eq!(prepared.changes().len(), 1);
        assert!(!root.join("settings.yaml").exists());

        // A 0.1 user layer holding an older default would win over the patch.
        let settings = root.join("settings.yaml");
        std::fs::write(
            &settings,
            "agent-default-model:\n  provider: deepseek-official\n  model: deepseek-flash\n",
        )
        .unwrap();
        let mut prepared = prepare_in(
            &root,
            DshProfile::Web,
            "https://yeschoy.com",
            "m",
            &ids,
            None,
        )
        .unwrap();
        assert_eq!(prepared.changes().len(), 2);
        prepared.commit().unwrap();
        let value: JsonValue = serde_yaml::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
        assert_eq!(value["agent-default-model"]["provider"], "yeschoy");

        // DSH 0.2 imports it into the booting profile and renames it.
        std::fs::rename(&settings, root.join("settings.yaml.imported")).unwrap();
        assert!(prepared.validate_existing().is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_held_profile_lock_blocks_the_write_and_a_stale_one_does_not() {
        let root = common::temporary_working_directory("dsh-lock").unwrap();
        let profile = root.join("profiles").join(DshProfile::Web.name());
        std::fs::create_dir_all(&profile).unwrap();
        let lock = profile.join("package.json.lock");
        std::fs::write(&lock, "1\n").unwrap();
        let mut prepared = prepare_in(
            &root,
            DshProfile::Web,
            "https://yeschoy.com",
            "m",
            &["m".into()],
            None,
        )
        .unwrap();
        assert!(prepared.commit().is_err());
        assert!(
            !prepared.patch_path.exists(),
            "nothing written under a live lock"
        );
        assert!(lock.exists(), "another writer's lock is not ours to remove");

        let file = std::fs::File::options().write(true).open(&lock).unwrap();
        file.set_modified(std::time::SystemTime::now() - Duration::from_secs(600))
            .unwrap();
        drop(file);
        prepared.commit().unwrap();
        assert!(!lock.exists(), "the lock is released after writing");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn desktop_writes_its_own_profile_and_the_shared_credentials_file() {
        let root = common::temporary_working_directory("dsh-desktop").unwrap();
        std::fs::write(
            root.join(CREDENTIALS_FILENAME),
            "version: 1\nrefs:\n  DEEPSEEK_API_KEY: sk-theirs\nrecords: {}\n",
        )
        .unwrap();
        let ids = vec!["m".to_string()];
        let mut prepared = prepare_in(
            &root,
            DshProfile::Desktop,
            "https://yeschoy.com",
            "m",
            &ids,
            Some("sk-ours"),
        )
        .unwrap();
        assert_eq!(prepared.changes().len(), 2);
        prepared.commit().unwrap();
        assert!(root.join("profiles/desktop/cordis.patch.yml").is_file());
        assert!(
            !root.join("profiles/web").exists(),
            "the web profile is not ours here"
        );
        let credentials: JsonValue =
            serde_yaml::from_slice(&std::fs::read(root.join(CREDENTIALS_FILENAME)).unwrap())
                .unwrap();
        assert_eq!(credentials["version"], 1);
        assert_eq!(credentials["refs"]["DEEPSEEK_API_KEY"], "sk-theirs");
        assert_eq!(credentials["refs"][KEY_REFERENCE], "sk-ours");
        assert!(credentials.get("records").is_some());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(root.join(CREDENTIALS_FILENAME))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(
                mode & 0o077,
                0,
                "DSH refuses a credentials file others can read"
            );
        }
        assert!(prepared.validate_existing().is_ok());

        // A rotated key is a changed configuration, not a silent success.
        let rotated = prepare_in(
            &root,
            DshProfile::Desktop,
            "https://yeschoy.com",
            "m",
            &ids,
            Some("sk-new"),
        )
        .unwrap();
        assert!(rotated.validate_existing().is_err());
        // The web profile never carries a key, the desktop one always does.
        assert!(prepare_in(
            &root,
            DshProfile::Desktop,
            "https://yeschoy.com",
            "m",
            &ids,
            None
        )
        .is_err());
        assert!(prepare_in(
            &root,
            DshProfile::Web,
            "https://yeschoy.com",
            "m",
            &ids,
            Some("k")
        )
        .is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn credentials_render_refuses_layouts_dsh_itself_refuses() {
        let created = render_credentials(None, "sk-ours").unwrap();
        let value: JsonValue = serde_yaml::from_slice(&created).unwrap();
        assert_eq!(
            value,
            json!({"version": 1, "refs": {KEY_REFERENCE: "sk-ours"}})
        );
        assert!(render_credentials(Some(b"\n"), "sk-ours").is_ok());
        // The pre-release flat layout, a future version, a non-mapping.
        assert!(render_credentials(Some(b"DEEPSEEK_API_KEY: sk\n"), "sk-ours").is_err());
        assert!(render_credentials(Some(b"version: 2\nrefs: {}\n"), "sk-ours").is_err());
        assert!(render_credentials(Some(b"- a\n"), "sk-ours").is_err());
        assert!(render_credentials(None, "").is_err());
    }

    #[test]
    fn restoring_credentials_takes_back_only_our_ref() {
        let path = Path::new("/x/.credentials.yaml");
        let after = render_credentials(None, "sk-ours").unwrap();
        // Created by us and untouched: the file goes.
        assert_eq!(
            restore_credentials(path, None, &after, &after)
                .unwrap()
                .unwrap(),
            (None, false)
        );
        // The user saved a key of their own since: it and `version` stay.
        let now =
            b"version: 1\nrefs:\n  YESCHOY_DSH_API_KEY: sk-ours\n  DEEPSEEK_API_KEY: sk-theirs\n";
        let (document, preserved) = restore_credentials(path, None, &after, now)
            .unwrap()
            .unwrap();
        assert_eq!(
            document,
            Some(json!({"version": 1, "refs": {"DEEPSEEK_API_KEY": "sk-theirs"}}))
        );
        assert!(!preserved);
        // A value that was there before comes back.
        let before = b"version: 1\nrefs:\n  YESCHOY_DSH_API_KEY: sk-older\n";
        let after = render_credentials(Some(before), "sk-ours").unwrap();
        let (document, _) = restore_credentials(path, Some(before), &after, &after)
            .unwrap()
            .unwrap();
        assert_eq!(
            document,
            Some(json!({"version": 1, "refs": {KEY_REFERENCE: "sk-older"}}))
        );
        // Someone changed our ref since: left alone, and reported.
        let now = b"version: 1\nrefs:\n  YESCHOY_DSH_API_KEY: sk-edited\n";
        let (_, preserved) = restore_credentials(path, Some(before), &after, now)
            .unwrap()
            .unwrap();
        assert!(preserved);
        assert!(restore_credentials(Path::new("/x/settings.yaml"), None, b"", b"").is_none());
    }

    fn restored(before: Option<&[u8]>, after: &[u8], current: &[u8]) -> (JsonValue, bool) {
        let (rows, preserved) = restore_profile_patch(
            Path::new("/x/profiles/web/cordis.patch.yml"),
            before,
            after,
            current,
        )
        .unwrap()
        .unwrap();
        (JsonValue::Array(rows), preserved)
    }

    #[test]
    fn restoring_the_patch_takes_back_only_what_we_own() {
        let after = render_profile_patch(None, "https://yeschoy.com", "m", &["m".into()]).unwrap();
        // Untouched since connecting: everything we added goes.
        let (rows, preserved) = restored(None, &after, &after);
        assert_eq!(rows, json!([]));
        assert!(!preserved);

        // The user added a provider to our row and picked a model of theirs.
        let mut now: JsonValue = serde_yaml::from_slice(&after).unwrap();
        now[0]["config"]["providers"]["local"] = json!({"baseURL": "http://127.0.0.1:11434/v1"});
        now[1]["config"] = json!({"provider": "local", "model": "qwen"});
        let current = serde_yaml::to_string(&now).unwrap();
        let (rows, preserved) = restored(None, &after, current.as_bytes());
        assert_eq!(
            rows,
            json!([
                {"id": LLM_ENTRY, "name": LLM_PLUGIN, "config": {"providers": {"local": {"baseURL": "http://127.0.0.1:11434/v1"}}}},
                {"id": DEFAULT_MODEL_ENTRY, "name": DEFAULT_MODEL_PLUGIN, "config": {"provider": "local", "model": "qwen"}}
            ])
        );
        assert!(
            !preserved,
            "their values are theirs, not a conflict with ours"
        );

        // Values that were there before connecting come back.
        let before = b"- id: agent-default-model\n  config: {provider: deepseek-official, model: deepseek-flash}\n- id: llm-pi-ai\n  config: {providers: {yeschoy: {reasoning: low}}}\n";
        let after =
            render_profile_patch(Some(before), "https://yeschoy.com", "m", &["m".into()]).unwrap();
        let (rows, _) = restored(Some(before), &after, &after);
        let expected: JsonValue = serde_yaml::from_slice(before).unwrap();
        assert_eq!(rows, expected);

        // Our provider changed since: it stays, and says so.
        let mut now: JsonValue = serde_yaml::from_slice(&after).unwrap();
        now[1]["config"]["providers"]["yeschoy"]["baseURL"] = "https://elsewhere/v1".into();
        let current = serde_yaml::to_string(&now).unwrap();
        let (rows, preserved) = restored(Some(before), &after, current.as_bytes());
        assert_eq!(
            rows[1]["config"]["providers"]["yeschoy"]["baseURL"],
            "https://elsewhere/v1"
        );
        assert!(preserved);

        assert!(restore_profile_patch(Path::new("/x/settings.yaml"), None, b"", b"").is_none());
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

    #[tokio::test]
    async fn startup_output_without_a_newline_still_has_a_hard_bound() {
        let bytes = vec![b'x'; STARTUP_OUTPUT_LIMIT + 1];
        let mut reader = BufReader::new(bytes.as_slice());
        let error = read_bounded_line(&mut reader, STARTUP_OUTPUT_LIMIT)
            .await
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);

        let bytes = vec![b'x'; STARTUP_OUTPUT_LIMIT];
        let mut reader = BufReader::new(bytes.as_slice());
        assert_eq!(
            read_bounded_line(&mut reader, STARTUP_OUTPUT_LIMIT)
                .await
                .unwrap()
                .len(),
            STARTUP_OUTPUT_LIMIT
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
    fn latest_release_candidate_uses_the_canonical_web_profile_without_a_prompt() {
        assert_eq!(
            start_arguments(),
            [
                "--profile",
                "web",
                "--host",
                "127.0.0.1",
                "--port",
                "0",
                "--no-open"
            ]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn node_wrapper_starts_from_a_gui_safe_child_path() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let root = common::temporary_working_directory("dsh-node-wrapper").unwrap();
        let shims = root.join("shims");
        let runtime_bin = root.join("runtime/bin");
        let package = root.join("runtime/lib/package");
        std::fs::create_dir_all(&shims).unwrap();
        std::fs::create_dir_all(&runtime_bin).unwrap();
        std::fs::create_dir_all(&package).unwrap();
        let target = package.join("dsh.js");
        std::fs::write(&target, b"#!/usr/bin/env node\nfixture\n").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
        let owned_wrapper = runtime_bin.join("dsh");
        symlink("../lib/package/dsh.js", &owned_wrapper).unwrap();
        let discovered_wrapper = shims.join("dsh");
        symlink(&owned_wrapper, &discovered_wrapper).unwrap();
        let node = runtime_bin.join("node");
        std::fs::write(
            &node,
            b"#!/bin/sh\nprintf 'dsh web: http://127.0.0.1:43127/?token=fixture_token\\n'\nexec /bin/sleep 10\n",
        )
        .unwrap();
        std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o700)).unwrap();

        let installation = ResolvedInstallation {
            path: discovered_wrapper,
        };
        let mut runtime = start_process(&installation, "synthetic-key", [7; 32])
            .await
            .unwrap();
        assert_eq!(runtime.url, "http://127.0.0.1:43127/?token=fixture_token");
        runtime.child.kill().await.unwrap();
        runtime.stdout_task.abort();
        drop(runtime);
        std::fs::remove_dir_all(root).unwrap();
    }
}
