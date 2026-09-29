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
        keys: &[(String, String)],
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
        let runtime = start_process(installation, keys, identity).await?;
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
    profile: DshProfile,
    /// `$DSH_HOME/profiles/<profile>/cordis.patch.yml`, always written.
    patch_path: PathBuf,
    /// `$DSH_HOME/.credentials.yaml`, written for the Desktop profile only.
    credentials_path: Option<PathBuf>,
    /// `$DSH_HOME/settings.yaml`, written only when it already exists.
    settings_path: Option<PathBuf>,
    origin: String,
    model: String,
    groups: Vec<Group>,
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

/// One model of a connection, with the billing group and scoped relay key the
/// relay issued for it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DshRoute {
    pub(crate) model_id: String,
    pub(crate) billing_group: String,
    pub(crate) key: String,
}

/// Every model of a stored credential, as routes. A credential without a
/// model set is its single default model.
pub(crate) fn routes(credential: &crate::tool_credentials::ToolCredential) -> Vec<DshRoute> {
    if credential.models.is_empty() {
        return vec![DshRoute {
            model_id: credential.model_id.clone(),
            billing_group: String::new(),
            key: credential.upstream_key().to_owned(),
        }];
    }
    credential
        .models
        .iter()
        .map(|route| DshRoute {
            model_id: route.model_id.clone(),
            billing_group: route.billing_group.clone(),
            key: route.api_key.clone(),
        })
        .collect()
}

/// The models that share one relay key, as one DSH provider.
///
/// A DSH provider has exactly one `apiKeyEnv`, and the relay scopes each key
/// to one billing group. One provider for the whole catalog sent the default
/// group's key for every model, so a model from any other group was listed
/// and then refused. The default model's group is `yeschoy` /
/// `YESCHOY_DSH_API_KEY`, as before; each further group is `yeschoy-2` /
/// `YESCHOY_DSH_API_KEY_2`, and so on.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Group {
    provider: String,
    reference: String,
    label: String,
    key: String,
    models: Vec<String>,
}

fn groups(default_model: &str, routes: &[DshRoute]) -> Result<Vec<Group>, ()> {
    let first = routes
        .iter()
        .find(|route| route.model_id == default_model)
        .ok_or(())?;
    let mut ordered: Vec<(&str, &str, Vec<String>)> =
        vec![(&first.billing_group, &first.key, Vec::new())];
    for route in routes {
        let slot = ordered
            .iter_mut()
            .find(|(group, key, _)| *group == route.billing_group && *key == route.key);
        match slot {
            Some((_, _, models)) => models.push(route.model_id.clone()),
            None => ordered.push((
                &route.billing_group,
                &route.key,
                vec![route.model_id.clone()],
            )),
        }
    }
    Ok(ordered
        .into_iter()
        .enumerate()
        .map(|(index, (group, key, models))| {
            let (provider, reference, label) = if index == 0 {
                (
                    "yeschoy".to_owned(),
                    KEY_REFERENCE.to_owned(),
                    "野菜API".to_owned(),
                )
            } else {
                let n = index + 1;
                let label = if group.is_empty() {
                    format!("野菜API {n}")
                } else {
                    format!("野菜API · {group}")
                };
                (
                    format!("yeschoy-{n}"),
                    format!("{KEY_REFERENCE}_{n}"),
                    label,
                )
            };
            Group {
                provider,
                reference,
                label,
                key: key.to_owned(),
                models,
            }
        })
        .collect())
}

/// `yeschoy`, or `yeschoy-<n>` for a further billing group.
fn is_owned_provider_id(id: &str) -> bool {
    id == "yeschoy"
        || id
            .strip_prefix("yeschoy-")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// `YESCHOY_DSH_API_KEY`, or `YESCHOY_DSH_API_KEY_<n>`.
fn is_owned_reference(reference: &str) -> bool {
    reference == KEY_REFERENCE
        || reference
            .strip_prefix(KEY_REFERENCE)
            .and_then(|rest| rest.strip_prefix('_'))
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// A provider this app wrote: our id, reading its key from our reference.
fn is_owned_provider(id: &str, provider: &Value) -> bool {
    is_owned_provider_id(id)
        && provider
            .get("apiKeyEnv")
            .and_then(Value::as_str)
            .is_some_and(is_owned_reference)
}

/// Put every group's provider into `providers`, and take out ours that the
/// current catalog no longer has a group for.
fn apply_providers(providers: &mut Mapping, origin: &str, groups: &[Group]) -> Result<(), ()> {
    let stale = providers
        .iter()
        .filter_map(|(id, provider)| {
            let id = id.as_str()?;
            (is_owned_provider(id, provider) && !groups.iter().any(|g| g.provider == id))
                .then(|| id.to_owned())
        })
        .collect::<Vec<_>>();
    for id in stale {
        providers.remove(id.as_str());
    }
    for group in groups {
        let previous = providers.get(group.provider.as_str()).cloned();
        providers.insert(
            Value::String(group.provider.clone()),
            yeschoy_provider(previous.as_ref(), origin, group)?,
        );
    }
    Ok(())
}

/// Our provider, over whatever the user already had under the same name.
///
/// Fields we do not own (`reasoning`, per-model budgets) survive, the same
/// rule `native_chat_catalog` applies to each model entry.
fn yeschoy_provider(previous: Option<&Value>, origin: &str, group: &Group) -> Result<Value, ()> {
    let mut provider = match previous {
        Some(Value::Mapping(existing)) => existing.clone(),
        _ => Mapping::new(),
    };
    provider.insert(
        Value::String("displayName".into()),
        Value::String(group.label.clone()),
    );
    provider.insert(
        Value::String("apiKeyEnv".into()),
        Value::String(group.reference.clone()),
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
        &group.models,
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
fn single(model: &str) -> Vec<Group> {
    groups(
        model,
        &[DshRoute {
            model_id: model.to_owned(),
            billing_group: String::new(),
            key: "sk-test".to_owned(),
        }],
    )
    .unwrap()
}

#[cfg(test)]
fn render(existing: Option<&[u8]>, origin: &str, model: &str) -> Result<Vec<u8>, ()> {
    render_catalog(existing, origin, model, &single(model))
}

/// DSH 0.1's `settings.yaml`: the user layer, which wins over the profile patch.
fn render_catalog(
    existing: Option<&[u8]>,
    origin: &str,
    model: &str,
    groups: &[Group],
) -> Result<Vec<u8>, ()> {
    let mut root = match existing {
        Some(bytes) if !bytes.is_empty() => {
            serde_yaml::from_slice::<Value>(bytes).map_err(|_| ())?
        }
        _ => Value::Mapping(Mapping::new()),
    };
    let root_map = mapping(&mut root)?;
    let llm = child_mapping(root_map, LLM_ENTRY)?;
    apply_providers(child_mapping(llm, "providers")?, origin, groups)?;
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
    groups: &[Group],
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
            apply_providers(
                child_mapping(row_config(row)?, "providers")?,
                origin,
                groups,
            )?;
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
    match profile {
        DshProfile::Desktop => paths.push(root.join(CREDENTIALS_FILENAME)),
        // `settings.yaml` is one file shared by every profile. Only the web
        // connection journals it: two connections recording the same file
        // would each take the other's values for the user's original and
        // write them back after that connection was gone. Desktop is 0.2,
        // which never reads it as settings anyway.
        DshProfile::Web => {
            let settings = root.join("settings.yaml");
            if settings.is_file() {
                paths.push(settings);
            }
        }
    }
    paths
}

/// `dsh --profile web`, which this app starts with the keys in its environment.
pub(crate) fn prepare_catalog(
    home: &Path,
    origin: &str,
    model: &str,
    routes: &[DshRoute],
) -> Result<Prepared, AdapterFailure> {
    let root = dsh_home(home, crate::shell_environment::var_os("DSH_HOME"))?;
    prepare_in(&root, DshProfile::Web, origin, model, routes)
}

/// The Harness home DeepSeek Harness Desktop itself resolves.
///
/// Desktop reads `DSH_HOME` from the environment it was launched with and
/// deliberately does not take `DSH_*` names from the login shell
/// (`LAUNCHER_OWNED_PREFIXES` in its `login-shell-environment.ts`). A Dock or
/// Start-menu launch inherits the same session environment as this app, so this
/// process's own value is Desktop's. The login shell's value, which `dsh web`
/// does honour, would send the configuration to a directory Desktop never reads.
fn desktop_dsh_home(home: &Path) -> Result<PathBuf, AdapterFailure> {
    dsh_home(home, std::env::var_os("DSH_HOME"))
}

/// DeepSeek Harness Desktop: its own profile, and the keys in DSH's credentials
/// file, because nothing this app starts stands between Desktop and the relay.
pub(crate) fn prepare_desktop_catalog(
    home: &Path,
    origin: &str,
    model: &str,
    routes: &[DshRoute],
) -> Result<Prepared, AdapterFailure> {
    let root = desktop_dsh_home(home)?;
    prepare_in(&root, DshProfile::Desktop, origin, model, routes)
}

fn prepare_in(
    root: &Path,
    profile: DshProfile,
    origin: &str,
    model: &str,
    routes: &[DshRoute],
) -> Result<Prepared, AdapterFailure> {
    let parse_failed = AdapterFailure::ConfigurationFailed("configuration_parse_failed");
    let invalid = AdapterFailure::ConfigurationFailed("invalid_request");
    let model_ids = routes
        .iter()
        .map(|route| route.model_id.clone())
        .collect::<Vec<_>>();
    crate::tool_adapters::common::validate_catalog(model, &model_ids)?;
    if routes.iter().any(|route| route.key.is_empty()) {
        return Err(invalid);
    }
    let groups = groups(model, routes).map_err(|_| invalid)?;
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
                render_profile_patch(before.as_deref(), origin, model, &groups)
            }
            Some(CREDENTIALS_FILENAME) => {
                credentials_path = Some(path.clone());
                render_credentials(before.as_deref(), &groups)
            }
            _ => {
                settings_path = Some(path.clone());
                render_catalog(before.as_deref(), origin, model, &groups)
            }
        }
        .map_err(|_| parse_failed)?;
        transaction
            .push_with_snapshot(path, before, after)
            .map_err(config_error)?;
    }
    Ok(Prepared {
        transaction,
        profile,
        patch_path: patch_path.ok_or(parse_failed)?,
        credentials_path,
        settings_path,
        origin: origin.to_owned(),
        model: model.to_owned(),
        groups,
    })
}

/// DSH's credentials document: `version: 1` and a `refs` map of reference name
/// to secret. Only our references are written, one per billing group, and ours
/// that no group uses any more are taken out; every other ref, and any
/// `records`, stay as they are.
///
/// A non-empty document without `version` is the pre-release flat layout,
/// which DSH itself refuses to start with and asks the user to migrate. It is
/// refused here as well rather than rewritten on their behalf.
fn render_credentials(existing: Option<&[u8]>, groups: &[Group]) -> Result<Vec<u8>, ()> {
    if groups.is_empty() || groups.iter().any(|group| group.key.is_empty()) {
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
    let stale = refs
        .keys()
        .filter_map(Value::as_str)
        .filter(|name| is_owned_reference(name) && !groups.iter().any(|g| g.reference == *name))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for name in stale {
        refs.remove(name.as_str());
    }
    for group in groups {
        refs.insert(
            Value::String(group.reference.clone()),
            Value::String(group.key.clone()),
        );
    }
    to_yaml_bytes(&Value::Mapping(root))
}

/// DSH's cross-process writer locks: an exclusively created lock file holding
/// the writer's PID, removed when the write is done.
///
/// - `<profile>/package.json.lock`: DSH's config editor, around every patch
///   write (the Models page, the model picker).
/// - `.credentials.yaml.lock`: DSH's credentials store, around every key change.
/// - `profiles/desktop/lock`: DeepSeek Harness Desktop's own profile lock, held
///   while it prepares the profile at startup and while its recovery renames
///   the patch away. Desktop owns that profile, so it is honoured as well.
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
        let mut locks = targets
            .into_iter()
            .map(|target| {
                let mut lock = target.into_os_string();
                lock.push(".lock");
                PathBuf::from(lock)
            })
            .collect::<Vec<_>>();
        if prepared.profile == DshProfile::Desktop {
            if let Some(profile) = prepared.patch_path.parent() {
                // Taken first, as Desktop takes it before touching the profile.
                locks.insert(0, profile.join("lock"));
            }
        }
        Self::take_all(locks)
    }

    fn take_all(locks: Vec<PathBuf>) -> Result<Self, AdapterFailure> {
        let mut held = Self(Vec::new());
        for lock in locks {
            // A directory that does not exist yet has no DSH writing into it.
            if lock.parent().is_some_and(Path::is_dir) {
                held.0.push(Self::take(lock)?);
            }
        }
        Ok(held)
    }

    fn take(lock: PathBuf) -> Result<PathBuf, AdapterFailure> {
        let busy = AdapterFailure::ConfigurationFailed("configuration_write_failed");
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

    fn provider_matches(&self, provider: &JsonValue, group: &Group) -> bool {
        provider["apiKeyEnv"].as_str() == Some(group.reference.as_str())
            && provider["api"].as_str() == Some("openai-completions")
            && provider["baseURL"].as_str()
                == Some(format!("{}/v1", self.origin.trim_end_matches('/')).as_str())
            && crate::tool_adapters::common::catalog_matches(
                &provider["models"],
                Some("id"),
                &group.models,
            )
            && provider["compat"]["supportsDeveloperRole"].as_bool() == Some(false)
            && provider["compat"]["maxTokensField"].as_str() == Some("max_tokens")
    }

    fn providers_match(&self, providers: &JsonValue) -> bool {
        self.groups
            .iter()
            .all(|group| self.provider_matches(&providers[group.provider.as_str()], group))
    }

    /// Right after our own write the default must be exactly ours. Later, any
    /// model of the connection is fine, selected through the provider that
    /// carries it: DSH's own model picker changes the default as it switches.
    fn default_matches(&self, defaults: &JsonValue, strict_default: bool) -> bool {
        let (provider, model) = (defaults["provider"].as_str(), defaults["model"].as_str());
        if strict_default {
            return provider == Some(self.groups[0].provider.as_str())
                && model == Some(self.model.as_str());
        }
        self.groups.iter().any(|group| {
            provider == Some(group.provider.as_str())
                && model.is_some_and(|model| group.models.iter().any(|id| id == model))
        })
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
            .is_some_and(|config| self.providers_match(&config["providers"]))
            && last_row_config(&rows, DEFAULT_MODEL_ENTRY)
                .is_some_and(|config| self.default_matches(config, strict_default));
        if !patch_correct {
            return Err(failed);
        }
        if let Some(path) = &self.credentials_path {
            let document = Self::read_yaml(path)?;
            let keys_correct = document["version"].as_u64() == Some(1)
                && self.groups.iter().all(|group| {
                    document["refs"][group.reference.as_str()].as_str() == Some(group.key.as_str())
                });
            if !keys_correct {
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
                let settings_correct = self.providers_match(&value[LLM_ENTRY]["providers"])
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
/// our providers and the default model's `provider` and `model`, each restored
/// to what it was (or removed) when it still holds what we wrote, and left in
/// place, reported, when the user has changed it since.
///
/// Rows are matched by content, not only by position: a row the user or DSH
/// inserted or deleted ahead of ours must not make every later row compare
/// with the wrong snapshot. For each value of ours still present, the snapshot
/// that wrote exactly it is found (the same position first), used once, and
/// its predecessor restored.
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
    Some((|| {
        let before = parse_patch_json(before)?;
        let after = parse_patch_json(Some(after))?;
        let mut now = parse_patch_json(Some(current))?;
        let providers_of = |rows: &[JsonValue]| -> Vec<JsonValue> {
            entry_rows(rows, LLM_ENTRY)
                .map(|row| row["config"]["providers"].clone())
                .collect()
        };
        let defaults_of = |rows: &[JsonValue]| -> Vec<JsonValue> {
            entry_rows(rows, DEFAULT_MODEL_ENTRY)
                .map(|row| row["config"].clone())
                .collect()
        };
        let (ours, theirs) = (providers_of(&after), providers_of(&before));
        let (default_ours, default_theirs) = (defaults_of(&after), defaults_of(&before));
        // Our provider ids: whatever we wrote under an owned id, in any row.
        let owned_ids = ours
            .iter()
            .filter_map(JsonValue::as_object)
            .flat_map(|providers| providers.keys())
            .filter(|id| is_owned_provider_id(id))
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let mut used_providers = std::collections::BTreeSet::new();
        let mut used_defaults = std::collections::BTreeSet::new();
        // The snapshot index whose value is `value`, same position first.
        let find = |candidates: &mut dyn Iterator<Item = usize>,
                    position: usize,
                    matches: &dyn Fn(usize) -> bool|
         -> Option<usize> {
            let all = candidates.collect::<Vec<_>>();
            all.iter()
                .copied()
                .find(|&j| j == position && matches(j))
                .or_else(|| all.iter().copied().find(|&j| matches(j)))
        };
        let pick = |value: &JsonValue| -> (JsonValue, JsonValue) {
            (value["provider"].clone(), value["model"].clone())
        };
        let mut preserved = false;
        let (mut llm_position, mut default_position) = (0, 0);
        let mut index = 0;
        while index < now.len() {
            let row = &mut now[index];
            let entry = if row.get("insert").is_none() {
                row["id"].as_str().map(str::to_owned)
            } else {
                None
            };
            if entry.as_deref() == Some(LLM_ENTRY) {
                let position = llm_position;
                llm_position += 1;
                let mut created = false;
                if let Some(providers) = row
                    .get_mut("config")
                    .and_then(|config| config.get_mut("providers"))
                    .and_then(JsonValue::as_object_mut)
                {
                    for id in &owned_ids {
                        let Some(current) = providers.get(id).cloned() else {
                            continue;
                        };
                        let matched = find(&mut (0..ours.len()), position, &|j| {
                            !used_providers.contains(&(j, id.clone()))
                                && ours[j].get(id) == Some(&current)
                        });
                        let Some(j) = matched else {
                            preserved = true;
                            continue;
                        };
                        used_providers.insert((j, id.clone()));
                        created |= j >= theirs.len();
                        match theirs.get(j).and_then(|providers| providers.get(id)) {
                            Some(value) => {
                                providers.insert(id.clone(), value.clone());
                            }
                            None => {
                                providers.remove(id);
                            }
                        }
                    }
                    if providers.is_empty() && created {
                        if let Some(config) = row["config"].as_object_mut() {
                            config.remove("providers");
                        }
                    }
                }
                let empty_config = row["config"]
                    .as_object()
                    .is_none_or(|config| config.is_empty());
                if empty_config && created {
                    now.remove(index);
                    continue;
                }
            } else if entry.as_deref() == Some(DEFAULT_MODEL_ENTRY) {
                let position = default_position;
                default_position += 1;
                let current = pick(&row["config"]);
                // Only `provider` and `model` are ours; another field the user
                // changed since (`reasoningEffort`) does not make it theirs.
                let matched = find(&mut (0..default_ours.len()), position, &|j| {
                    !used_defaults.contains(&j) && pick(&default_ours[j]) == current
                });
                match matched {
                    Some(j) => {
                        used_defaults.insert(j);
                        match default_theirs.get(j) {
                            // A row we added: the entry falls back to its default.
                            None => {
                                now.remove(index);
                                continue;
                            }
                            Some(JsonValue::Null) => {
                                if let Some(row) = row.as_object_mut() {
                                    row.remove("config");
                                }
                            }
                            Some(theirs) => {
                                if let Some(config) = row["config"].as_object_mut() {
                                    for field in ["provider", "model"] {
                                        match theirs.get(field) {
                                            Some(value) => {
                                                config.insert(field.into(), value.clone());
                                            }
                                            None => {
                                                config.remove(field);
                                            }
                                        }
                                    }
                                    // Activation dropped an effort that belonged
                                    // to their model; it comes back unless the
                                    // user has set one since.
                                    if let Some(effort) = theirs.get("reasoningEffort") {
                                        if default_ours[j].get("reasoningEffort").is_none() {
                                            config
                                                .entry("reasoningEffort")
                                                .or_insert_with(|| effort.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        if row["config"]["provider"]
                            .as_str()
                            .is_some_and(is_owned_provider_id)
                        {
                            preserved = true;
                        }
                    }
                }
            }
            index += 1;
        }
        Ok((now, preserved))
    })())
}

fn parse_patch_json(bytes: Option<&[u8]>) -> Result<Vec<JsonValue>, ()> {
    parse_patch(bytes)?
        .into_iter()
        .map(|row| serde_json::to_value(row).map_err(|_| ()))
        .collect()
}

/// The rows that configure `id`, in order; `insert` rows add entries instead.
fn entry_rows<'a>(rows: &'a [JsonValue], id: &'a str) -> impl Iterator<Item = &'a JsonValue> {
    rows.iter()
        .filter(move |row| row["id"].as_str() == Some(id) && row.get("insert").is_none())
}

/// Remove every value in a profile patch that only this app writes: a
/// `yeschoy` provider reading its key from `YESCHOY_DSH_API_KEY`, and a
/// default-model row selecting that provider.
///
/// Used where there is no record of what the file held before we touched it:
/// a connection from before the recovery journal, and a patch that DSH 0.2
/// filled by importing our old `settings.yaml`. Without it that imported copy
/// of our configuration would be taken for the user's own and put back on
/// disconnect, after the key it needs was deleted.
///
/// A default-model row selecting us is removed whole: its config replaces the
/// entry's, and `provider` and `model` are required there.
pub(crate) fn strip_owned_patch(bytes: &[u8]) -> Result<Vec<u8>, ()> {
    let mut rows = parse_patch(Some(bytes))?;
    let original = rows.clone();
    rows.retain_mut(|row| {
        if is_entry_row(row, LLM_ENTRY) {
            let Some(providers) = row
                .get_mut("config")
                .and_then(|config| config.get_mut("providers"))
                .and_then(Value::as_mapping_mut)
            else {
                return true;
            };
            let ours = providers
                .iter()
                .filter_map(|(id, provider)| {
                    let id = id.as_str()?;
                    is_owned_provider(id, provider).then(|| id.to_owned())
                })
                .collect::<Vec<_>>();
            if ours.is_empty() {
                return true;
            }
            for id in ours {
                providers.remove(id.as_str());
            }
            if providers.is_empty() {
                if let Some(config) = row.get_mut("config").and_then(Value::as_mapping_mut) {
                    config.remove("providers");
                }
            }
            row.get("config")
                .and_then(Value::as_mapping)
                .is_some_and(|config| !config.is_empty())
        } else if is_entry_row(row, DEFAULT_MODEL_ENTRY) {
            !row.get("config")
                .and_then(|config| config.get("provider"))
                .and_then(Value::as_str)
                .is_some_and(is_owned_provider_id)
        } else {
            true
        }
    });
    if rows == original {
        return Ok(bytes.to_vec());
    }
    to_yaml_bytes(&Value::Sequence(rows))
}

/// DSH's writer locks for files that are about to be written outside an
/// adapter transaction, such as a disconnect restoring them. Paths that are
/// not DSH's are ignored.
pub(crate) fn lock_for_restore(paths: &[PathBuf]) -> Result<impl Drop, AdapterFailure> {
    let mut locks = Vec::new();
    for path in paths {
        match path.file_name().and_then(|name| name.to_str()) {
            Some(PATCH_FILENAME) => {
                let Some(profile) = path.parent() else {
                    continue;
                };
                if profile.file_name().and_then(|name| name.to_str())
                    == Some(DshProfile::Desktop.name())
                {
                    locks.push(profile.join("lock"));
                }
                locks.push(profile.join("package.json.lock"));
            }
            Some(CREDENTIALS_FILENAME) => {
                let mut lock = path.clone().into_os_string();
                lock.push(".lock");
                locks.push(PathBuf::from(lock));
            }
            _ => {}
        }
    }
    ProfileLock::take_all(locks)
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
        let theirs = before.map(parse).transpose()?.unwrap_or(JsonValue::Null);
        let ours = parse(after)?;
        let mut now = parse(current)?;
        let mut preserved = false;
        // One reference per billing group: every one of ours we wrote.
        let references = ours["refs"]
            .as_object()
            .into_iter()
            .flat_map(|refs| refs.keys())
            .filter(|name| is_owned_reference(name))
            .cloned()
            .collect::<Vec<_>>();
        if let Some(refs) = now.get_mut("refs").and_then(JsonValue::as_object_mut) {
            for name in &references {
                match (refs.get(name), ours["refs"].get(name)) {
                    (Some(current), Some(ours)) if current == ours => {
                        match theirs["refs"].get(name) {
                            Some(value) => {
                                refs.insert(name.clone(), value.clone());
                            }
                            None => {
                                refs.remove(name);
                            }
                        }
                    }
                    (Some(_), _) => preserved = true,
                    (None, _) => {}
                }
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
    keys: &[(String, String)],
    identity: [u8; 32],
) -> Result<DshRuntime, AdapterFailure> {
    let mut command = Command::new(&installation.path);
    command
        .args(start_arguments())
        .envs(keys.iter().map(|(name, value)| (name, value)))
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

/// Each billing group's reference and key, for the process environment.
pub(crate) fn key_environment(
    credential: &crate::tool_credentials::ToolCredential,
) -> Result<Vec<(String, String)>, AdapterFailure> {
    Ok(groups(&credential.model_id, &routes(credential))
        .map_err(|_| AdapterFailure::LaunchFailed)?
        .into_iter()
        .map(|group| (group.reference, group.key))
        .collect())
}

pub(crate) async fn open_existing(
    state: &DshRuntimeState,
    installation: &ResolvedInstallation,
    credential: &crate::tool_credentials::ToolCredential,
) -> Result<(), AdapterFailure> {
    let keys = key_environment(credential)?;
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
    let identity = runtime_identity(&installation.path, &patch, &keys);
    let url = state.ensure_running(installation, &keys, identity).await?;
    open_browser(&url)
}

/// What a running DSH cannot pick up without a restart.
///
/// Both 0.1 and 0.2 watch the profile patch and reload model settings on the
/// next request, so a catalog, default model or endpoint change does not end a
/// live session. The keys are different: they reach DSH through the process
/// environment, which only a new process gets.
fn runtime_identity(installation: &Path, patch: &Path, keys: &[(String, String)]) -> [u8; 32] {
    let mut fingerprint = Sha256::new();
    let paths = [
        installation.to_string_lossy().into_owned(),
        patch.to_string_lossy().into_owned(),
    ];
    let parts = paths.iter().map(String::as_bytes).chain(
        keys.iter()
            .flat_map(|(name, value)| [name.as_bytes(), value.as_bytes()]),
    );
    for part in parts {
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

    fn routes_for(ids: &[String], key: &str) -> Vec<DshRoute> {
        ids.iter()
            .map(|id| DshRoute {
                model_id: id.clone(),
                billing_group: String::new(),
                key: key.to_owned(),
            })
            .collect()
    }

    fn patch_ids(
        existing: Option<&[u8]>,
        origin: &str,
        model: &str,
        ids: &[String],
    ) -> Result<Vec<u8>, ()> {
        render_profile_patch(
            existing,
            origin,
            model,
            &groups(model, &routes_for(ids, "sk-test"))?,
        )
    }

    fn catalog_ids(
        existing: Option<&[u8]>,
        origin: &str,
        model: &str,
        ids: &[String],
    ) -> Result<Vec<u8>, ()> {
        render_catalog(
            existing,
            origin,
            model,
            &groups(model, &routes_for(ids, "sk-test"))?,
        )
    }

    fn prepare_ids(
        root: &Path,
        profile: DshProfile,
        origin: &str,
        model: &str,
        ids: &[String],
        key: Option<&str>,
    ) -> Result<Prepared, AdapterFailure> {
        prepare_in(
            root,
            profile,
            origin,
            model,
            &routes_for(ids, key.unwrap_or("sk-test")),
        )
    }

    fn credentials_for(existing: Option<&[u8]>, key: &str) -> Result<Vec<u8>, ()> {
        let mut groups = single("m");
        groups[0].key = key.to_owned();
        render_credentials(existing, &groups)
    }

    fn one_key(key: &str) -> Vec<(String, String)> {
        vec![(KEY_REFERENCE.to_owned(), key.to_owned())]
    }

    fn identity_for(installation: &Path, patch: &Path, key: &str) -> [u8; 32] {
        runtime_identity(installation, patch, &one_key(key))
    }

    // The process fixtures that use these are POSIX shell scripts.
    #[cfg(unix)]
    impl DshRuntimeState {
        async fn ensure_running_key(
            &self,
            installation: &ResolvedInstallation,
            key: &str,
            identity: [u8; 32],
        ) -> Result<String, AdapterFailure> {
            self.ensure_running(installation, &one_key(key), identity)
                .await
        }
    }

    #[cfg(unix)]
    async fn start_process_key(
        installation: &ResolvedInstallation,
        key: &str,
        identity: [u8; 32],
    ) -> Result<DshRuntime, AdapterFailure> {
        start_process(installation, &one_key(key), identity).await
    }

    #[test]
    fn ru043_dsh_catalog_efforts_preserve_existing_budget() {
        let existing = b"llm-pi-ai:\n  providers:\n    yeschoy:\n      reasoning: low\n      models:\n        - id: deepseek-v4-flash\n          maxTokens: 4096\n          contextWindow: 65536\n";
        let bytes = catalog_ids(
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
        let mut prepared = prepare_ids(
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
        let before = identity_for(&installation, &path, "synthetic-local");

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
            identity_for(&installation, &path, "synthetic-local")
        );
        assert_ne!(before, identity_for(&installation, &path, "rotated-local"));

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
        let a = identity_for(&installation.path, &config, "synthetic-local");
        let b = identity_for(&installation.path, &config, "synthetic-local");
        let state = DshRuntimeState::default();
        state
            .ensure_running_key(&installation, "synthetic-local", a)
            .await
            .unwrap();
        let pid = state.runtime.lock().await.as_ref().unwrap().child.id();
        state
            .ensure_running_key(&installation, "synthetic-local", b)
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
            state.ensure_running_key(&installation, "synthetic-key", [1; 32]),
            state.ensure_running_key(&installation, "synthetic-key", [1; 32]),
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
            .ensure_running_key(&installation, "synthetic-key", [1; 32])
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
            .ensure_running_key(&installation, "synthetic-key", [1; 32])
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
            .ensure_running_key(&installation, "synthetic-new-key", [2; 32])
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
        let mut prepared = prepare_ids(
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
        let bytes = patch_ids(
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
        let bytes = patch_ids(None, "https://yeschoy.com", "m", &["m".into()]).unwrap();
        let rows: JsonValue = serde_yaml::from_slice(&bytes).unwrap();
        assert_eq!(rows[0]["id"], LLM_ENTRY);
        assert_eq!(rows[0]["name"], LLM_PLUGIN);
        assert_eq!(rows[1]["id"], DEFAULT_MODEL_ENTRY);
        assert_eq!(rows[1]["name"], DEFAULT_MODEL_PLUGIN);
        for empty in [&b""[..], b"   \n", b"~\n", b"[]\n"] {
            assert!(patch_ids(Some(empty), "https://yeschoy.com", "m", &["m".into()]).is_ok());
        }
        for foreign in [
            &b"llm-pi-ai: {}\n"[..],
            b"- id: llm-pi-ai\n  config: [1]\n",
            b"- [\n",
        ] {
            assert!(patch_ids(Some(foreign), "https://yeschoy.com", "m", &["m".into()]).is_err());
        }
        // An `insert` row adds entries; it is not the entry's configuration.
        let insert = b"- id: llm-pi-ai\n  insert: [{name: x}]\n";
        let bytes = patch_ids(Some(insert), "https://yeschoy.com", "m", &["m".into()]).unwrap();
        let rows: JsonValue = serde_yaml::from_slice(&bytes).unwrap();
        assert!(rows[0].get("config").is_none());
        assert_eq!(rows[1]["id"], LLM_ENTRY);
    }

    #[test]
    fn settings_yaml_is_written_only_while_it_exists_and_may_disappear() {
        let root = common::temporary_working_directory("dsh-settings-layer").unwrap();
        let ids = vec!["m".to_string()];
        let prepared = prepare_ids(
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
        let mut prepared = prepare_ids(
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
        let mut prepared = prepare_ids(
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
        let mut prepared = prepare_ids(
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
        let rotated = prepare_ids(
            &root,
            DshProfile::Desktop,
            "https://yeschoy.com",
            "m",
            &ids,
            Some("sk-new"),
        )
        .unwrap();
        assert!(rotated.validate_existing().is_err());
        // The web profile never writes the credentials file: its keys go into
        // the environment of the process this app starts. An empty key is
        // refused rather than written.
        let web = prepare_ids(
            &root,
            DshProfile::Web,
            "https://yeschoy.com",
            "m",
            &ids,
            None,
        )
        .unwrap();
        assert!(web
            .changes()
            .iter()
            .all(|change| !change.path.ends_with(CREDENTIALS_FILENAME)));
        assert!(prepare_ids(
            &root,
            DshProfile::Desktop,
            "https://yeschoy.com",
            "m",
            &ids,
            Some("")
        )
        .is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn credentials_render_refuses_layouts_dsh_itself_refuses() {
        let created = credentials_for(None, "sk-ours").unwrap();
        let value: JsonValue = serde_yaml::from_slice(&created).unwrap();
        assert_eq!(
            value,
            json!({"version": 1, "refs": {KEY_REFERENCE: "sk-ours"}})
        );
        assert!(credentials_for(Some(b"\n"), "sk-ours").is_ok());
        // The pre-release flat layout, a future version, a non-mapping.
        assert!(credentials_for(Some(b"DEEPSEEK_API_KEY: sk\n"), "sk-ours").is_err());
        assert!(credentials_for(Some(b"version: 2\nrefs: {}\n"), "sk-ours").is_err());
        assert!(credentials_for(Some(b"- a\n"), "sk-ours").is_err());
        assert!(credentials_for(None, "").is_err());
    }

    #[test]
    fn restoring_credentials_takes_back_only_our_ref() {
        let path = Path::new("/x/.credentials.yaml");
        let after = credentials_for(None, "sk-ours").unwrap();
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
        let after = credentials_for(Some(before), "sk-ours").unwrap();
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

    #[test]
    fn desktop_waits_for_its_own_profile_lock_too() {
        let root = common::temporary_working_directory("dsh-desktop-lock").unwrap();
        let profile = root.join("profiles/desktop");
        std::fs::create_dir_all(&profile).unwrap();
        // Desktop holds this while it prepares the profile or recovers it.
        let lock = profile.join("lock");
        std::fs::write(&lock, "1\n").unwrap();
        let ids = vec!["m".to_string()];
        let mut prepared = prepare_ids(
            &root,
            DshProfile::Desktop,
            "https://yeschoy.com",
            "m",
            &ids,
            Some("k"),
        )
        .unwrap();
        assert!(prepared.commit().is_err());
        assert!(!prepared.patch_path.exists());
        assert!(!root.join(CREDENTIALS_FILENAME).exists());
        assert!(lock.exists(), "Desktop's lock is not ours to remove");
        std::fs::remove_file(&lock).unwrap();
        prepared.commit().unwrap();
        assert!(!lock.exists(), "our hold on it is released");
        // The web profile has no such lock.
        std::fs::create_dir_all(root.join("profiles/web")).unwrap();
        std::fs::write(root.join("profiles/web/lock"), "1\n").unwrap();
        prepare_ids(
            &root,
            DshProfile::Web,
            "https://yeschoy.com",
            "m",
            &ids,
            None,
        )
        .unwrap()
        .commit()
        .unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stripping_removes_only_what_this_app_writes() {
        let ours = patch_ids(
            Some(b"- id: llm-pi-ai\n  config:\n    providers:\n      local: {baseURL: 'http://127.0.0.1:11434/v1'}\n- id: ui-settings\n  config: {theme: dark}\n"),
            "https://yeschoy.com",
            "m",
            &["m".into()],
        )
        .unwrap();
        let stripped: JsonValue =
            serde_yaml::from_slice(&strip_owned_patch(&ours).unwrap()).unwrap();
        assert_eq!(
            stripped,
            json!([
                {"id": "llm-pi-ai", "config": {"providers": {"local": {"baseURL": "http://127.0.0.1:11434/v1"}}}},
                {"id": "ui-settings", "config": {"theme": "dark"}}
            ])
        );
        // A provider the user named `yeschoy` with their own key reference stays.
        let theirs = b"- id: llm-pi-ai\n  config:\n    providers:\n      yeschoy: {apiKeyEnv: MY_KEY}\n- id: agent-default-model\n  config: {provider: deepseek-official, model: deepseek-flash}\n";
        assert_eq!(strip_owned_patch(theirs).unwrap(), theirs.to_vec());
        assert!(strip_owned_patch(b"llm-pi-ai: {}\n").is_err());
    }

    #[test]
    fn repeated_rows_are_restored_against_their_own_snapshots() {
        let before = b"- id: agent-default-model\n  config: {provider: a, model: one}\n- id: llm-pi-ai\n  config: {providers: {yeschoy: {reasoning: low}}}\n- id: agent-default-model\n  config: {provider: b, model: two}\n- id: llm-pi-ai\n  config: {providers: {yeschoy: {reasoning: high}}}\n";
        let after = patch_ids(Some(before), "https://yeschoy.com", "m", &["m".into()]).unwrap();
        // Any edit elsewhere sends restore down the semantic path.
        let mut now: JsonValue = serde_yaml::from_slice(&after).unwrap();
        now.as_array_mut()
            .unwrap()
            .push(json!({"id": "ui-settings", "config": {"theme": "dark"}}));
        let current = serde_yaml::to_string(&now).unwrap();
        let (rows, preserved) = restored(Some(before), &after, current.as_bytes());
        let mut expected: JsonValue = serde_yaml::from_slice(before).unwrap();
        expected
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "ui-settings", "config": {"theme": "dark"}}));
        assert_eq!(rows, expected);
        assert!(!preserved);
    }

    #[test]
    fn an_insert_row_is_not_a_configuration_row_we_found() {
        let before = b"- id: llm-pi-ai\n  insert: [{name: extra}]\n";
        let after = patch_ids(Some(before), "https://yeschoy.com", "m", &["m".into()]).unwrap();
        let mut now: JsonValue = serde_yaml::from_slice(&after).unwrap();
        now.as_array_mut()
            .unwrap()
            .push(json!({"id": "ui-settings", "config": {"theme": "dark"}}));
        let current = serde_yaml::to_string(&now).unwrap();
        let (rows, _) = restored(Some(before), &after, current.as_bytes());
        assert_eq!(
            rows,
            json!([
                {"id": "llm-pi-ai", "insert": [{"name": "extra"}]},
                {"id": "ui-settings", "config": {"theme": "dark"}}
            ])
        );
    }

    #[test]
    fn restore_waits_for_dsh_writers_on_the_files_it_writes() {
        let root = common::temporary_working_directory("dsh-restore-lock").unwrap();
        let web = root.join("profiles/web");
        let desktop = root.join("profiles/desktop");
        std::fs::create_dir_all(&web).unwrap();
        std::fs::create_dir_all(&desktop).unwrap();
        let paths = vec![
            web.join(PATCH_FILENAME),
            desktop.join(PATCH_FILENAME),
            root.join(CREDENTIALS_FILENAME),
            root.join("settings.yaml"),
        ];
        {
            let _held = lock_for_restore(&paths).unwrap();
            for lock in [
                web.join("package.json.lock"),
                desktop.join("package.json.lock"),
                desktop.join("lock"),
                root.join(".credentials.yaml.lock"),
            ] {
                assert!(lock.exists(), "{}", lock.display());
            }
            assert!(!web.join("lock").exists());
            // A second writer is refused while they are held.
            assert!(lock_for_restore(&paths[..1]).is_err());
        }
        assert!(!web.join("package.json.lock").exists());
        assert!(!desktop.join("lock").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn two_groups() -> Vec<DshRoute> {
        [
            ("glm-5.3", "默认分组", "sk-default"),
            ("deepseek-v4-flash", "国模特价分组", "sk-cheap"),
            ("glm-5.3-air", "默认分组", "sk-default"),
        ]
        .into_iter()
        .map(|(model, group, key)| DshRoute {
            model_id: model.into(),
            billing_group: group.into(),
            key: key.into(),
        })
        .collect()
    }

    #[test]
    fn each_billing_group_is_its_own_provider_with_its_own_key() {
        let root = common::temporary_working_directory("dsh-groups").unwrap();
        let mut prepared = prepare_in(
            &root,
            DshProfile::Desktop,
            "https://yeschoy.com",
            "glm-5.3",
            &two_groups(),
        )
        .unwrap();
        prepared.commit().unwrap();
        let rows = patch_rows(&prepared.patch_path);
        let providers = &rows[0]["config"]["providers"];
        assert_eq!(providers["yeschoy"]["apiKeyEnv"], KEY_REFERENCE);
        let ids = |provider: &JsonValue| {
            provider["models"]
                .as_array()
                .unwrap()
                .iter()
                .map(|model| model["id"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&providers["yeschoy"]), ["glm-5.3", "glm-5.3-air"]);
        assert_eq!(providers["yeschoy-2"]["apiKeyEnv"], "YESCHOY_DSH_API_KEY_2");
        assert_eq!(
            providers["yeschoy-2"]["displayName"],
            "野菜API · 国模特价分组"
        );
        assert_eq!(ids(&providers["yeschoy-2"]), ["deepseek-v4-flash"]);
        assert_eq!(rows[1]["config"]["provider"], "yeschoy");
        let credentials: JsonValue =
            serde_yaml::from_slice(&std::fs::read(root.join(CREDENTIALS_FILENAME)).unwrap())
                .unwrap();
        assert_eq!(credentials["refs"][KEY_REFERENCE], "sk-default");
        assert_eq!(credentials["refs"]["YESCHOY_DSH_API_KEY_2"], "sk-cheap");
        // Switching models inside the connection in DSH's own picker is fine.
        let mut rows = rows;
        rows[1]["config"] = json!({"provider": "yeschoy-2", "model": "deepseek-v4-flash"});
        std::fs::write(&prepared.patch_path, serde_yaml::to_string(&rows).unwrap()).unwrap();
        assert!(prepared.validate_existing().is_ok());

        // Reconnecting with one group takes the other group's provider and
        // reference out, not just leaves them stale.
        let single = two_groups().into_iter().take(1).collect::<Vec<_>>();
        let mut prepared = prepare_in(
            &root,
            DshProfile::Desktop,
            "https://yeschoy.com",
            "glm-5.3",
            &single,
        )
        .unwrap();
        prepared.commit().unwrap();
        let rows = patch_rows(&prepared.patch_path);
        assert!(rows[0]["config"]["providers"].get("yeschoy-2").is_none());
        let credentials: JsonValue =
            serde_yaml::from_slice(&std::fs::read(root.join(CREDENTIALS_FILENAME)).unwrap())
                .unwrap();
        assert!(credentials["refs"].get("YESCHOY_DSH_API_KEY_2").is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_web_runtime_gets_every_groups_key() {
        let credential = crate::tool_credentials::ToolCredential {
            api_key: "sk-default".into(),
            origin: "https://yeschoy.com".into(),
            model_id: "glm-5.3".into(),
            local_gateway_token: None,
            codex_transport: None,
            claude_transport: None,
            models: two_groups()
                .into_iter()
                .map(|route| crate::tool_credentials::ToolModelRoute {
                    model_id: route.model_id,
                    billing_group: route.billing_group,
                    api_key: route.key,
                    origin: "https://yeschoy.com".into(),
                    claude_transport: None,
                    codex_transport: None,
                })
                .collect(),
        };
        assert_eq!(
            key_environment(&credential).unwrap(),
            [
                (KEY_REFERENCE.to_owned(), "sk-default".to_owned()),
                ("YESCHOY_DSH_API_KEY_2".to_owned(), "sk-cheap".to_owned()),
            ]
        );
    }

    #[test]
    fn desktop_never_journals_the_shared_settings_file() {
        let root = common::temporary_working_directory("dsh-desktop-settings").unwrap();
        std::fs::write(root.join("settings.yaml"), "a: 1\n").unwrap();
        let desktop = prepare_ids(
            &root,
            DshProfile::Desktop,
            "https://yeschoy.com",
            "m",
            &["m".into()],
            Some("k"),
        )
        .unwrap();
        assert!(desktop
            .changes()
            .iter()
            .all(|change| !change.path.ends_with("settings.yaml")));
        let web = prepare_ids(
            &root,
            DshProfile::Web,
            "https://yeschoy.com",
            "m",
            &["m".into()],
            None,
        )
        .unwrap();
        assert!(web
            .changes()
            .iter()
            .any(|change| change.path.ends_with("settings.yaml")));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restore_still_finds_our_rows_after_one_is_inserted_ahead() {
        let before = b"- id: llm-pi-ai\n  config: {providers: {yeschoy: {reasoning: low}}}\n- id: agent-default-model\n  config: {provider: a, model: one}\n";
        let after = patch_ids(Some(before), "https://yeschoy.com", "m", &["m".into()]).unwrap();
        let mut now: JsonValue = serde_yaml::from_slice(&after).unwrap();
        // DSH's Models page added a row of its own in front of ours.
        now.as_array_mut().unwrap().insert(
            0,
            json!({"id": "llm-pi-ai", "config": {"providers": {"local": {"baseURL": "x"}}}}),
        );
        let current = serde_yaml::to_string(&now).unwrap();
        let (rows, preserved) = restored(Some(before), &after, current.as_bytes());
        assert!(!preserved);
        assert_eq!(
            rows,
            json!([
                {"id": "llm-pi-ai", "config": {"providers": {"local": {"baseURL": "x"}}}},
                {"id": "llm-pi-ai", "config": {"providers": {"yeschoy": {"reasoning": "low"}}}},
                {"id": "agent-default-model", "config": {"provider": "a", "model": "one"}}
            ])
        );
    }

    #[test]
    fn restore_takes_back_our_default_even_when_the_user_changed_its_effort() {
        let before = b"- id: agent-default-model\n  config: {provider: a, model: one, reasoningEffort: max}\n";
        let after = patch_ids(Some(before), "https://yeschoy.com", "m", &["m".into()]).unwrap();
        let mut now: JsonValue = serde_yaml::from_slice(&after).unwrap();
        let default = now
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["id"] == DEFAULT_MODEL_ENTRY)
            .unwrap();
        default["config"]["reasoningEffort"] = "low".into();
        let current = serde_yaml::to_string(&now).unwrap();
        let (rows, preserved) = restored(Some(before), &after, current.as_bytes());
        assert!(!preserved);
        let default = rows
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == DEFAULT_MODEL_ENTRY)
            .unwrap();
        // Ours goes back to theirs; the effort the user just chose stays.
        assert_eq!(
            default["config"],
            json!({"provider": "a", "model": "one", "reasoningEffort": "low"})
        );
        // Untouched since, their own effort comes back too.
        let (rows, _) = restored(Some(before), &after, &after);
        let default = rows
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == DEFAULT_MODEL_ENTRY)
            .unwrap();
        assert_eq!(default["config"]["reasoningEffort"], "max");
    }

    #[test]
    fn stripping_and_restoring_credentials_cover_every_group() {
        let two = patch_ids(None, "https://yeschoy.com", "glm-5.3", &["glm-5.3".into()]).unwrap();
        let mut rows: JsonValue = serde_yaml::from_slice(&two).unwrap();
        rows[0]["config"]["providers"]["yeschoy-2"] =
            json!({"apiKeyEnv": "YESCHOY_DSH_API_KEY_2", "baseURL": "https://yeschoy.com/v1"});
        rows[1]["config"]["provider"] = "yeschoy-2".into();
        let bytes = serde_yaml::to_string(&rows).unwrap();
        let stripped: JsonValue =
            serde_yaml::from_slice(&strip_owned_patch(bytes.as_bytes()).unwrap()).unwrap();
        assert_eq!(stripped, json!([]));

        let groups = groups("glm-5.3", &two_groups()).unwrap();
        let after = render_credentials(None, &groups).unwrap();
        let (document, _) =
            restore_credentials(Path::new("/x/.credentials.yaml"), None, &after, &after)
                .unwrap()
                .unwrap();
        assert_eq!(document, None);
        assert!(!is_owned_reference("YESCHOY_DSH_API_KEY_X"));
        assert!(is_owned_provider_id("yeschoy-12") && !is_owned_provider_id("yeschoy-local"));
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
        let after = patch_ids(None, "https://yeschoy.com", "m", &["m".into()]).unwrap();
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
        let after = patch_ids(Some(before), "https://yeschoy.com", "m", &["m".into()]).unwrap();
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
        let mut runtime = start_process_key(&installation, "synthetic-key", [7; 32])
            .await
            .unwrap();
        assert_eq!(runtime.url, "http://127.0.0.1:43127/?token=fixture_token");
        runtime.child.kill().await.unwrap();
        runtime.stdout_task.abort();
        drop(runtime);
        std::fs::remove_dir_all(root).unwrap();
    }
}
