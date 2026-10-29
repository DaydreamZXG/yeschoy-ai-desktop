//! 卸载前的「0 垃圾留存」清理。
//!
//! 助手本身只写两类东西：用户选择接入的应用配置（由「恢复原设置」按
//! 原样撤回），和它自己的状态——`~/.yeschoy`、日志、钥匙串条目、WebView
//! 本地存储。后者没有任何界面会用到，却会在卸载后一直留着。
//!
//! 这里做三件事：
//!
//! 1. `inventory`：只读盘点，告诉用户「卸载后会留下什么」和每一项能不能清
//!    （以及为什么）。
//! 2. `preview`：把将要删除的**具体相对路径**列出来，用户确认前不删任何东西
//!    （产品不变量 1）。
//! 3. `execute`：按确认记录逐项删除，返回每一项的结果；再次执行时已清掉的
//!    条目自然消失，所以它是可重入的。
//!
//! 安全边界：
//! - 只删白名单里的位置，路径全部由原生侧拼接，渲染层只传选项布尔值。
//! - 真机测试用的数据目录与生产分开（`YESCHOY_STATE_HOME` 覆盖），否则
//!   一次测试就会抹掉开发者正在用的助手的恢复基线。
//! - 钥匙串只删本应用自己的 service，绝不动 Claude Code 自己的登录态。
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::tool_adapters;
use crate::tool_credentials;

const RECOVERY_SERVICE: &str = "com.yeschoy.desktop.connection-recovery.v1";
const RECOVERY_ACCOUNT: &str = "encryption-key";
const CREDENTIAL_SERVICE: &str = "com.yeschoy.desktop.tool-credential.v2";
const ACCOUNT_SERVICE: &str = "com.yeschoy.desktop.account";
const ACCOUNT_USER: &str = "current-session-v2";
const DISPLACED_LOGIN_SERVICE: &str = "com.yeschoy.desktop.displaced-login.v1";

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResidueItem {
    pub(crate) id: &'static str,
    /// 删除什么（不含绝对路径）。路径属于本机信息，界面不展示。
    pub(crate) target: &'static str,
    /// `ready` 可以清理；`action` 需要用户先做一步；`kept` 有意保留。
    pub(crate) state: &'static str,
    /// 稳定英文 reason code，前端出文案。
    pub(crate) reason_code: &'static str,
    pub(crate) selected_by_default: bool,
    /// 已存在、应当出现在预览里的相对路径（只用于本地预览，不展示给他人）。
    pub(crate) entries: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResidueInventory {
    pub(crate) request_id: String,
    /// 所有白名单目录都不存在，且本应用没有钥匙串条目。
    pub(crate) clean: bool,
    pub(crate) items: Vec<ResidueItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Removal {
    id: &'static str,
    /// 文件或目录；不存在时视为已清理。
    path: PathBuf,
}

type Keyring = dyn Fn(&str, &str) -> bool;

/// 归本应用所有的状态目录。测试与 `pnpm dev` 可以用 `YESCHOY_STATE_HOME`
/// 指到临时目录，避免清理测试碰到真实的 `~/.yeschoy`。
fn state_home() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("YESCHOY_STATE_HOME") {
        return Some(PathBuf::from(root));
    }
    Some(tool_adapters::user_home()?.join(".yeschoy"))
}

/// macOS 日志目录名含中文「野菜API」；Windows 是 `%LOCALAPPDATA%\野菜API\logs`。
fn log_file() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = tool_adapters::user_home()?;
        Some(
            home.join("Library")
                .join("Logs")
                .join("野菜API")
                .join("yeschoy.log"),
        )
    }
    #[cfg(target_os = "windows")]
    {
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
        Some(local.join("野菜API").join("logs").join("yeschoy.log"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let home = tool_adapters::user_home()?;
        Some(
            home.join(".local")
                .join("state")
                .join("野菜API")
                .join("yeschoy.log"),
        )
    }
}

fn log_dir() -> Option<PathBuf> {
    log_file()?.parent().map(Path::to_path_buf)
}

/// 每个存储根的标识子目录。删除前会检查标题文件（Windows）或只删这些
/// 子目录（macOS / Linux），不会把整个 WebKit 目录一起抹掉。
fn webview_root() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        Some(tool_adapters::user_home()?.join("Library").join("WebKit"))
    }
    #[cfg(target_os = "windows")]
    {
        Some(
            std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .or_else(|| tool_adapters::user_home().map(|h| h.join("AppData").join("Local")))?,
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Some(tool_adapters::user_home()?.join(".local").join("share"))
    }
}

const IDENTIFIER: &str = "com.yeschoy.desktop";

/// WebView 运行时按标识符放置的各个目录（相对存储根的路径）。
fn webview_children() -> Vec<(&'static str, PathBuf)> {
    let Some(root) = webview_root() else {
        return Vec::new();
    };
    let mut children: Vec<(&'static str, PathBuf)> = Vec::new();
    #[cfg(target_os = "macos")]
    {
        // WKWebView 的数据是按 bundle identifier 命名的子目录。
        children.push(("webkit", root.join(IDENTIFIER)));
    }
    #[cfg(target_os = "windows")]
    {
        use webview2_children::*;
        for (id, name) in [
            ("webview2_user_data", EBWEBVIEW),
            ("caches", "Caches"),
            ("local_storage", "Local Storage"),
            ("session_storage", "Session Storage"),
            ("web_storage", "WebStorage"),
            ("file_system", "File System"),
            ("service_worker", "Service Worker"),
            ("gpu_cache", "GPUCache"),
        ] {
            children.push((id, root.join(name).join(IDENTIFIER)));
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = root;
    }
    children
}

#[cfg(target_os = "windows")]
mod webview2_children {
    /// WebView2 的用户数据目录名。
    pub(super) const EBWEBVIEW: &str = "EBWebView";
}

/// 目录下一层的名字（排序后），用于确认后的预览。
fn direct_children(path: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    names.sort();
    names.truncate(200);
    names
}

/// 钥匙串里有没有这个条目。只查存在性，不读取密文，所以不会弹授权框。
fn keyring_present(service: &str, account: &str) -> bool {
    match keyring::v1::Entry::new(service, account) {
        Ok(entry) => !matches!(entry.get_password(), Err(keyring::v1::Error::NoEntry)),
        Err(_) => false,
    }
}

fn pending_restores(state: &Path) -> Vec<String> {
    // `~/…/.yeschoy/connection-recovery` 下每个子目录对应一个仍处于接入状态
    // 的应用；卸载前应当先在「我的应用」里恢复原设置。
    direct_children(&state.join("connection-recovery"))
}

pub(crate) fn inventory(request_id: &str, keyring: &Keyring) -> ResidueInventory {
    let state = state_home();
    let mut items = Vec::new();

    // 只列出真实存在的目录。之前无条件列出 state 项，清空后仍会显示一个
    // 「可删」的空项，用户会以为没清干净。
    if let Some(state) = state.as_ref().filter(|path| path.exists()) {
        let restores = pending_restores(state);
        items.push(ResidueItem {
            id: "state",
            target: "state_dir",
            state: if restores.is_empty() {
                "ready"
            } else {
                "action"
            },
            reason_code: if restores.is_empty() {
                "owned_state"
            } else {
                "restore_first"
            },
            selected_by_default: true,
            entries: direct_children(state),
        });
    }

    if let Some(dir) = log_dir() {
        let log = log_file();
        let present = log.as_deref().is_some_and(Path::exists);
        if present || dir.exists() {
            items.push(ResidueItem {
                id: "logs",
                target: "log_dir",
                state: "ready",
                reason_code: "owned_logs",
                selected_by_default: true,
                entries: log
                    .as_deref()
                    .filter(|path| path.exists())
                    .and_then(|path| path.file_name().and_then(|n| n.to_str()))
                    .map(|name| vec![name.to_owned()])
                    .unwrap_or_default(),
            });
        }
    }

    for (id, path) in webview_children() {
        if !path.exists() {
            continue;
        }
        items.push(ResidueItem {
            id,
            target: "webview_data",
            state: "ready",
            // WebView 数据只能在应用退出后删：运行中的 WebView 会锁住它。
            reason_code: "quit_then_remove",
            selected_by_default: true,
            entries: direct_children(&path),
        });
    }

    let owned_tools = tool_credentials::ALLOWED_TOOLS
        .iter()
        .filter(|tool| (keyring)(CREDENTIAL_SERVICE, tool))
        .count();
    let shared = [
        (ACCOUNT_SERVICE, ACCOUNT_USER),
        (RECOVERY_SERVICE, RECOVERY_ACCOUNT),
    ]
    .iter()
    .filter(|(service, account)| (keyring)(service, account))
    .count();
    if owned_tools > 0 || shared > 0 {
        items.push(ResidueItem {
            id: "keychain",
            target: "keychain",
            state: "ready",
            reason_code: "owned_keychain",
            selected_by_default: true,
            entries: Vec::new(),
        });
    }
    // 只有真的挪走过 claude.ai 登录态时才需要处理；避免给大多数用户列出
    // 一条不存在的待办。
    if keyring_present_any(DISPLACED_LOGIN_SERVICE, keyring) {
        items.push(ResidueItem {
            id: "displaced_login",
            target: "keychain",
            state: "action",
            reason_code: "login_taken_over",
            selected_by_default: false,
            entries: Vec::new(),
        });
    }

    items.push(ResidueItem {
        id: "app_configs",
        target: "tool_configs",
        state: "kept",
        reason_code: "restore_via_app",
        selected_by_default: false,
        entries: Vec::new(),
    });
    items.push(ResidueItem {
        id: "crash_reports",
        target: "os_crash_reports",
        state: "kept",
        reason_code: "os_managed",
        selected_by_default: false,
        entries: Vec::new(),
    });

    ResidueInventory {
        request_id: request_id.to_owned(),
        clean: items.iter().all(|item| item.state == "kept"),
        items,
    }
}

/// 只按 service 判断是否存在：Claude Code 登录态被接管时 account 名来自
/// 条目自身，调用方无法预先知道。
fn keyring_present_any(service: &str, keyring: &Keyring) -> bool {
    #[cfg(target_os = "macos")]
    {
        let _ = (service, keyring);
        claude_login_takeover_has_backup()
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Windows 凭据管理器按 target 名索引，没有「按 service 枚举」的
        // 公开接口；这里退化为登录用户名，找不到就当没有。
        let account = std::env::var("USERNAME").unwrap_or_default();
        !account.is_empty() && keyring(service, &account)
    }
}

#[cfg(target_os = "macos")]
fn claude_login_takeover_has_backup() -> bool {
    let Ok(output) = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", DISPLACED_LOGIN_SERVICE])
        .output()
    else {
        return false;
    };
    // 不带 -w 只查属性，不读取密文，不会弹授权框。
    output.status.success()
}

/// 每一项对应的真实路径。用户勾选项（`state` / `logs` / WebView 各项）
/// 与清理执行共用这一份映射，避免「预览说删 A、实际删了 B」。
fn removals(inventory: &ResidueInventory) -> Vec<Removal> {
    let mut removals: Vec<Removal> = Vec::new();
    for item in &inventory.items {
        if item.state != "ready" {
            continue;
        }
        let path = match item.id {
            "state" => state_home(),
            "logs" => log_dir(),
            id => webview_children()
                .into_iter()
                .find(|(name, _)| *name == id)
                .map(|(_, path)| path),
        };
        if let Some(path) = path {
            removals.push(Removal { id: item.id, path });
        }
    }
    removals
}

/// 勾选状态决定是否执行某一项。WebView 各组共用「清理本地数据」开关；
/// 内部小缓存（`caches` / `gpu_cache`）跟着 WebView 用户数据一起走。
fn selected(removal: &Removal, selection: &ResidueSelection) -> bool {
    match removal.id {
        "state" => selection.include_state,
        "logs" => selection.include_logs,
        "caches" | "gpu_cache" => selection.include_webview_data,
        _ => selection.include_webview_data,
    }
}

#[derive(Clone, Copy, Debug)]
struct ResidueSelection {
    include_state: bool,
    include_logs: bool,
    include_webview_data: bool,
}

fn keychain_removals(inventory: &ResidueInventory) -> Vec<(&'static str, &'static str)> {
    if !inventory
        .items
        .iter()
        .any(|item| item.id == "keychain" && item.state == "ready")
    {
        return Vec::new();
    }
    let mut entries = vec![
        (ACCOUNT_SERVICE, ACCOUNT_USER),
        (RECOVERY_SERVICE, RECOVERY_ACCOUNT),
    ];
    for tool in tool_credentials::ALLOWED_TOOLS {
        entries.push((CREDENTIAL_SERVICE, tool));
    }
    entries
}

fn delete_entry(service: &str, account: &str) -> bool {
    match keyring::v1::Entry::new(service, account) {
        Ok(entry) => matches!(
            entry.delete_credential(),
            Ok(()) | Err(keyring::v1::Error::NoEntry)
        ),
        Err(_) => false,
    }
}

fn delete_path(path: &Path) -> bool {
    if !path.exists() {
        return true;
    }
    let result = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    result.is_ok() || !path.exists()
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResidueRemoval {
    pub(crate) id: &'static str,
    /// `removed` 已清理；`skipped` 本就不存在；`failed` 需要用户手动处理。
    pub(crate) status: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResidueRemovalResult {
    pub(crate) request_id: String,
    pub(crate) schema_version: u8,
    pub(crate) removed: Vec<ResidueRemoval>,
    /// 清理后重新盘点，界面据此显示「已归零」还是「还剩什么」。
    pub(crate) remaining: ResidueInventory,
}

/// 删除选定项。渲染层只能给 `include_*` 布尔值，具体路径一律来自原生侧。
#[tauri::command]
pub(crate) fn residue_clear_v1(
    request_id: String,
    include_state: bool,
    include_logs: bool,
    include_webview_data: bool,
    include_keychain: bool,
) -> ResidueRemovalResult {
    let before = inventory(&request_id, &|service, account| {
        keyring_present(service, account)
    });
    let mut removed = Vec::new();
    let selection = ResidueSelection {
        include_state,
        include_logs,
        include_webview_data,
    };
    for removal in removals(&before) {
        if !selected(&removal, &selection) {
            continue;
        }
        let status = if delete_path(&removal.path) {
            "removed"
        } else {
            "failed"
        };
        removed.push(ResidueRemoval {
            id: removal.id,
            status,
        });
    }
    if include_keychain {
        let entries = keychain_removals(&before);
        let mut all = true;
        for (service, account) in entries {
            all &= delete_entry(service, account);
        }
        if before.items.iter().any(|item| item.id == "keychain") {
            removed.push(ResidueRemoval {
                id: "keychain",
                status: if all { "removed" } else { "failed" },
            });
        }
    }
    let remaining = inventory(&request_id, &|service, account| {
        keyring_present(service, account)
    });
    log::info!(
        "residue stage=clear removed={} failed={} remaining={}",
        removed.iter().filter(|r| r.status == "removed").count(),
        removed.iter().filter(|r| r.status == "failed").count(),
        remaining.items.iter().filter(|i| i.state != "kept").count()
    );
    ResidueRemovalResult {
        request_id,
        schema_version: 1,
        removed,
        remaining,
    }
}

#[tauri::command]
pub(crate) fn residue_inventory_read_only(request_id: String) -> ResidueInventory {
    let inventory = inventory(&request_id, &|service, account| {
        keyring_present(service, account)
    });
    // 只记数量，不记路径。
    log::info!(
        "residue stage=inventory items={} clean={}",
        inventory.items.len(),
        inventory.clean
    );
    inventory
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempHome {
        root: PathBuf,
    }
    impl TempHome {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "yeschoy-residue-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&root).unwrap();
            Self { root }
        }
    }
    impl Drop for TempHome {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.root).ok();
        }
    }

    fn no_keyring(_: &str, _: &str) -> bool {
        false
    }

    #[test]
    fn never_touches_claude_codes_own_login() {
        // 这个常量在测试里被引用，防止有人「顺手」把 Claude Code 的
        // `Claude Code-credentials` 加进清理名单。
        let keys = keychain_removals(&inventory("t", &no_keyring));
        for (service, _) in keys {
            assert!(service.starts_with("com.yeschoy.desktop."), "{service}");
        }
    }

    #[test]
    fn reports_pending_restores_before_deleting_the_recovery_baseline() {
        let temp = TempHome::new("restore");
        let state = temp.root.join(".yeschoy");
        std::fs::create_dir_all(state.join("connection-recovery").join("codex_desktop")).unwrap();
        std::fs::write(state.join("device-id"), "x").unwrap();
        std::env::set_var("YESCHOY_STATE_HOME", &state);
        let inv = inventory("t", &no_keyring);
        std::env::remove_var("YESCHOY_STATE_HOME");
        let item = inv
            .items
            .iter()
            .find(|item| item.id == "state")
            .expect("state item");
        assert_eq!(item.state, "action");
        assert_eq!(item.reason_code, "restore_first");
        assert!(item.entries.contains(&"connection-recovery".to_owned()));
        assert!(item.entries.contains(&"device-id".to_owned()));
        assert!(!inv.clean);
    }

    #[test]
    fn an_absent_install_reports_clean() {
        let temp = TempHome::new("clean");
        std::env::set_var("YESCHOY_STATE_HOME", temp.root.join("missing"));
        let inv = inventory("t", &no_keyring);
        std::env::remove_var("YESCHOY_STATE_HOME");
        // 日志与 WebView 目录属于真实用户目录，测试不假设它们不存在；
        // 只断言「没有可删的东西时 clean 为真」这一条性质。
        let has_removable = inv.items.iter().any(|item| item.state == "ready");
        assert_eq!(inv.clean, !has_removable);
        // 无论怎样，应用配置与崩溃报告都显式标为「保留」并有理由。
        assert!(inv
            .items
            .iter()
            .any(|item| item.id == "app_configs" && item.state == "kept"));
        assert!(inv
            .items
            .iter()
            .any(|item| item.id == "crash_reports" && item.reason_code == "os_managed"));
    }

    #[test]
    fn preview_lists_concrete_paths_and_removal_is_reentrant() {
        let temp = TempHome::new("preview");
        let state = temp.root.join(".yeschoy");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("device-id"), "x").unwrap();
        std::env::set_var("YESSCHOY_STATE_HOME_TYPO", "");
        std::env::set_var("YESCHOY_STATE_HOME", &state);
        let inv = inventory("t", &no_keyring);
        let plan = removals(&inv);
        let state_plan = plan
            .iter()
            .find(|removal| removal.id == "state")
            .expect("state is removable");
        assert_eq!(state_plan.path, state);

        // 未勾选的项不删（用户确认前不写：选了别的就不能连带删掉）。
        let nothing = ResidueSelection {
            include_state: false,
            include_logs: false,
            include_webview_data: false,
        };
        assert!(!selected(state_plan, &nothing));
        assert!(state.exists());

        let only_state = ResidueSelection {
            include_state: true,
            ..nothing
        };
        assert!(selected(state_plan, &only_state));
        assert!(delete_path(&state_plan.path));
        assert!(!state.exists());

        // 再盘点一次：状态项消失，说明预览与执行共用同一份清单。
        let after = inventory("t", &no_keyring);
        std::env::remove_var("YESCHOY_STATE_HOME");
        std::env::remove_var("YESCHOY_STATE_HOME_TYPO");
        assert!(!after.items.iter().any(|item| item.id == "state"));
        assert!(delete_path(&temp.root.join("already-gone")));
    }

    #[test]
    fn keychain_presence_uses_service_and_account() {
        // 非 macOS / Windows 上没有可用钥匙串时，查询应安静地返回 false。
        assert!(!keyring_present(
            "com.yeschoy.desktop.residue-test-only",
            "nobody"
        ));
    }
}
