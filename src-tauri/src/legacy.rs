//! 改名之前（野菜API、`com.yeschoy.desktop`）留在这台电脑上的东西，搬到新名字下面。
//!
//! 老用户升级后不该重新登录、重新接入，所以：
//!
//!   * 数据目录 `~/.yeschoy` 整个改名成 `~/.kaifan`。改不动（旧版还开着、文件被
//!     占用）就这次继续用旧目录，下次启动再搬。
//!   * 系统密码库里的条目读到时再搬：新名字下没有，就去旧名字下找；抄到新名字、
//!     读回一致之后才删旧的。抄不过去也照样把值交出去，登录不会因此掉。
//!   * 写进别的应用里的“取密钥命令”带着客户端自己的路径。程序改名或挪了位置，
//!     那条命令就指向一个不存在的文件；`follow_helper` 把它改成现在的位置。
//!
//! 这里不写任何密钥内容到日志。
use keyring::v1::{Entry, Error as KeyringError};
use std::path::{Path, PathBuf};

pub(crate) const IDENTIFIER: &str = "com.kaifan.desktop";
pub(crate) const OLD_IDENTIFIER: &str = "com.yeschoy.desktop";
const HOME: &str = ".kaifan";
const OLD_HOME: &str = ".yeschoy";
/// 改名前的可执行文件名（不带扩展名）。
const OLD_BINARY: &str = "yeschoy-desktop";

/// 客户端自己的数据目录。旧目录还没搬过来时就是旧目录，绝不会两边各建一份。
pub(crate) fn data_root(home: &Path) -> PathBuf {
    let current = home.join(HOME);
    if current.symlink_metadata().is_ok() {
        return current;
    }
    let old = home.join(OLD_HOME);
    if is_plain_directory(&old) {
        old
    } else {
        current
    }
}

fn is_plain_directory(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
}

/// 目标不存在、来源是个普通目录时改名。返回是否真的搬了。
fn adopt_directory(old: &Path, current: &Path) -> bool {
    if current.symlink_metadata().is_ok() || !is_plain_directory(old) {
        return false;
    }
    if let Some(parent) = current.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::rename(old, current).is_ok()
}

/// 搬数据目录。旧版客户端如果正在改某个应用的设置，它会一直拿着这把锁，做完
/// 还要把结果记回旧目录；这时候搬走，那条结果就记到了一个新建的空目录里，这边
/// 会把一次已经做完的接入当成没做完再改回去。所以它忙着就不搬，下次启动再说。
fn adopt_home(old: &Path, current: &Path) -> bool {
    if current.symlink_metadata().is_ok() || !is_plain_directory(old) {
        return false;
    }
    let lock = old.join("connection-recovery").join("operation.lock");
    let guard = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock)
    {
        Ok(file) if file.try_lock().is_ok() => Some(file),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Ok(_) | Err(_) => return false,
    };
    // Windows 上目录里有文件开着就改不了名，先放手；别的系统拿着锁改名，旧版插不进来。
    #[cfg(windows)]
    drop(guard);
    let moved = adopt_directory(old, current);
    #[cfg(not(windows))]
    drop(guard);
    moved
}

/// 启动最早的一步：任何模块读写数据目录、拿锁、建窗口之前。
pub(crate) fn move_in() {
    if let Some(home) = crate::tool_adapters::user_home() {
        let moved = adopt_home(&home.join(OLD_HOME), &home.join(HOME));
        log::info!("legacy stage=home moved={moved}");
        #[cfg(target_os = "macos")]
        adopt_directory(
            &home.join("Library/WebKit").join(OLD_IDENTIFIER),
            &home.join("Library/WebKit").join(IDENTIFIER),
        );
    }
    // 系统按应用标识分给我们的目录：记住的站点、更新标记、安装包缓存，以及
    // 界面自己记的偏好（语言、静音、当前角色、挑过的型号和价格方案）。
    let mut roots = vec![dirs::data_dir(), dirs::data_local_dir(), dirs::cache_dir()];
    roots.dedup();
    for root in roots.into_iter().flatten() {
        adopt_directory(&root.join(OLD_IDENTIFIER), &root.join(IDENTIFIER));
    }
}

pub(crate) struct KeyFault;

pub(crate) trait Keys {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, KeyFault>;
    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), KeyFault>;
    fn delete(&self, service: &str, account: &str) -> Result<(), KeyFault>;
    /// 搬旧密钥、写密钥、删密钥互相错开用的锁，放手就解开。别的进程（应用来取
    /// 密钥时起的那个）也认它。拿不到是 `None`。
    fn hold(&self) -> Option<Box<dyn std::any::Any>>;
}

pub(crate) struct OsKeys;

impl Keys for OsKeys {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, KeyFault> {
        match Entry::new(service, account)
            .map_err(|_| KeyFault)?
            .get_password()
        {
            Ok(value) => Ok(Some(value)),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(_) => Err(KeyFault),
        }
    }
    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), KeyFault> {
        Entry::new(service, account)
            .map_err(|_| KeyFault)?
            .set_password(value)
            .map_err(|_| KeyFault)
    }
    fn delete(&self, service: &str, account: &str) -> Result<(), KeyFault> {
        match Entry::new(service, account)
            .map_err(|_| KeyFault)?
            .delete_credential()
        {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(_) => Err(KeyFault),
        }
    }
    fn hold(&self) -> Option<Box<dyn std::any::Any>> {
        // 只用已经有的数据目录，不新建：取密钥的进程比搬目录那一步还早，它要是
        // 先建了新目录，旧目录就再也搬不过来。
        let root = data_root(&crate::tool_adapters::user_home()?);
        if !is_plain_directory(&root) {
            return None;
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("keys.lock"))
            .ok()?;
        file.lock().ok()?;
        Some(Box::new(file))
    }
}

/// 改名前同一类条目用的名字。
pub(crate) fn old_service(service: &str) -> Option<String> {
    service
        .strip_prefix(IDENTIFIER)
        .map(|rest| format!("{OLD_IDENTIFIER}{rest}"))
}

/// 读一条密钥；新名字下没有就从旧名字下搬过来。
pub(crate) fn read_secret_in(
    keys: &impl Keys,
    service: &str,
    account: &str,
) -> Result<Option<String>, KeyFault> {
    if let Some(value) = keys.get(service, account)? {
        return Ok(Some(value));
    }
    let Some(old) = old_service(service) else {
        return Ok(None);
    };
    let Some(value) = keys.get(&old, account)? else {
        return Ok(None);
    };
    // 从读到旧的到抄过去之间，别的进程可能刚写了新的、或者刚删掉：照着手里这份
    // 旧的抄，就把人家的盖掉了。所以拿着锁再看一遍才抄；拿不到锁这次就不搬，
    // 值照样交出去。
    let Some(guard) = keys.hold() else {
        return Ok(Some(value));
    };
    if let Some(newer) = keys.get(service, account)? {
        return Ok(Some(newer));
    }
    let Some(value) = keys.get(&old, account)? else {
        return Ok(None);
    };
    // 旧版客户端如果还留着这条，它拿着旧的续期凭据去换新的，会把这边挤下线。
    // 所以搬完就删；但只在新的那条确实读得回来之后。
    let copied = keys.set(service, account, &value).is_ok()
        && keys.get(service, account).ok().flatten().as_deref() == Some(value.as_str());
    drop(guard);
    if copied {
        let _ = keys.delete(&old, account);
    }
    Ok(Some(value))
}

/// 写一条密钥。旧名字下那条随之作废，免得以后删掉新的时它又冒出来。
pub(crate) fn write_secret_in(
    keys: &impl Keys,
    service: &str,
    account: &str,
    value: &str,
) -> Result<(), KeyFault> {
    let guard = keys.hold();
    keys.set(service, account, value)?;
    drop(guard);
    if let Some(old) = old_service(service) {
        let _ = keys.delete(&old, account);
    }
    Ok(())
}

/// 删一条密钥，新旧两个名字下都删：旧的删不掉就算没删成，否则下次读又搬回来。
pub(crate) fn delete_secret_in(
    keys: &impl Keys,
    service: &str,
    account: &str,
) -> Result<(), KeyFault> {
    let _guard = keys.hold();
    keys.delete(service, account)?;
    match old_service(service) {
        Some(old) => keys.delete(&old, account),
        None => Ok(()),
    }
}

pub(crate) fn read_secret(service: &str, account: &str) -> Result<Option<String>, KeyFault> {
    read_secret_in(&OsKeys, service, account)
}

pub(crate) fn write_secret(service: &str, account: &str, value: &str) -> Result<(), KeyFault> {
    write_secret_in(&OsKeys, service, account, value)
}

pub(crate) fn delete_secret(service: &str, account: &str) -> Result<(), KeyFault> {
    delete_secret_in(&OsKeys, service, account)
}

/// 取密钥命令里的客户端路径该不该跟到 `current`：它已经不存在了，或者它是改名前
/// 的那个程序（旧版还装着也一样——它读不到已经搬走的密钥）。
pub(crate) fn should_follow(old: &Path, current: &Path) -> bool {
    old != current
        && (!old.exists()
            || old
                .file_stem()
                .is_some_and(|stem| stem.eq_ignore_ascii_case(OLD_BINARY)))
}

/// 拆开 `'<路径>' credential-helper <应用>` 这样的命令，取出路径。
fn helper_executable(command: &str, tool: &str) -> Option<(String, &'static str)> {
    let (quoted, kind) = [" gateway-credential-helper ", " credential-helper "]
        .into_iter()
        .find_map(|kind| {
            command
                .strip_suffix(tool)
                .and_then(|rest| rest.strip_suffix(kind))
                .map(|quoted| (quoted, kind))
        })?;
    let path = if let Some(inner) = quoted
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
    {
        inner.replace("'\\''", "'")
    } else if let Some(inner) = quoted
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        inner.to_owned()
    } else {
        return None;
    };
    (!path.is_empty()).then_some((path, kind))
}

/// 这条取密钥命令换成 `current` 之后的样子；不用换、认不出来都是 `None`。
/// `quote` 是当前平台给路径加引号的规则，加不了引号时返回空串。
pub(crate) fn followed_command(
    command: &str,
    tool: &str,
    current: &Path,
    quote: impl Fn(&str) -> String,
) -> Option<String> {
    let (old, kind) = helper_executable(command, tool)?;
    if !should_follow(Path::new(&old), current) {
        return None;
    }
    let quoted = quote(current.to_str()?);
    (!quoted.is_empty()).then(|| format!("{quoted}{kind}{tool}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::BTreeMap};

    #[derive(Default)]
    struct Memory {
        entries: RefCell<BTreeMap<(String, String), String>>,
        refuse_writes_to: Option<&'static str>,
        /// 别的进程在我们读完旧的、拿到锁之前做的事。
        meanwhile: RefCell<Option<Meanwhile>>,
        /// 锁拿不到。
        locked: bool,
    }
    type Meanwhile = Box<dyn FnOnce(&Memory)>;

    impl Memory {
        fn with(entries: &[(&str, &str, &str)]) -> Self {
            let memory = Self::default();
            for (service, account, value) in entries {
                memory.entries.borrow_mut().insert(
                    ((*service).to_owned(), (*account).to_owned()),
                    (*value).to_owned(),
                );
            }
            memory
        }
        fn has(&self, service: &str, account: &str) -> bool {
            self.entries
                .borrow()
                .contains_key(&(service.to_owned(), account.to_owned()))
        }
    }

    impl Keys for Memory {
        fn get(&self, service: &str, account: &str) -> Result<Option<String>, KeyFault> {
            Ok(self
                .entries
                .borrow()
                .get(&(service.to_owned(), account.to_owned()))
                .cloned())
        }
        fn set(&self, service: &str, account: &str, value: &str) -> Result<(), KeyFault> {
            if self.refuse_writes_to == Some(service) {
                return Err(KeyFault);
            }
            self.entries
                .borrow_mut()
                .insert((service.to_owned(), account.to_owned()), value.to_owned());
            Ok(())
        }
        fn delete(&self, service: &str, account: &str) -> Result<(), KeyFault> {
            self.entries
                .borrow_mut()
                .remove(&(service.to_owned(), account.to_owned()));
            Ok(())
        }
        fn hold(&self) -> Option<Box<dyn std::any::Any>> {
            if self.locked {
                return None;
            }
            if let Some(act) = self.meanwhile.borrow_mut().take() {
                act(self);
            }
            Some(Box::new(()))
        }
    }

    const NEW: &str = "com.kaifan.desktop.account";
    const OLD: &str = "com.yeschoy.desktop.account";

    fn read(keys: &Memory) -> Option<String> {
        read_secret_in(keys, NEW, "session").ok().flatten()
    }

    #[test]
    fn an_old_login_is_adopted_once_and_the_old_copy_is_removed() {
        let keys = Memory::with(&[(OLD, "session", "refresh-1")]);
        assert_eq!(read(&keys).as_deref(), Some("refresh-1"));
        assert!(keys.has(NEW, "session"));
        assert!(!keys.has(OLD, "session"));
        // 再读一次：已经搬完，结果一样。
        assert_eq!(read(&keys).as_deref(), Some("refresh-1"));
    }

    #[test]
    fn a_failed_copy_keeps_the_old_login_usable_and_retries_later() {
        let mut keys = Memory::with(&[(OLD, "session", "refresh-1")]);
        keys.refuse_writes_to = Some(NEW);
        assert_eq!(read(&keys).as_deref(), Some("refresh-1"));
        assert!(keys.has(OLD, "session"));
        assert!(!keys.has(NEW, "session"));
        keys.refuse_writes_to = None;
        assert_eq!(read(&keys).as_deref(), Some("refresh-1"));
        assert!(!keys.has(OLD, "session"));
    }

    #[test]
    fn the_new_entry_wins_and_nothing_is_touched_when_there_is_no_old_one() {
        let keys = Memory::with(&[(NEW, "session", "new"), (OLD, "session", "stale")]);
        assert_eq!(read(&keys).as_deref(), Some("new"));
        assert!(keys.has(OLD, "session"));
        assert_eq!(read(&Memory::default()), None);
    }

    /// 正要把旧的抄过去时，别的进程刚写了新的（重新接入）或刚删掉（改回原样）：
    /// 不拿手里那份旧的去盖。
    #[test]
    fn adopting_never_overwrites_what_another_process_just_did() {
        let keys = Memory::with(&[(OLD, "session", "stale")]);
        *keys.meanwhile.borrow_mut() = Some(Box::new(|keys: &Memory| {
            keys.entries
                .borrow_mut()
                .insert((NEW.to_owned(), "session".to_owned()), "fresh".to_owned());
            keys.entries
                .borrow_mut()
                .remove(&(OLD.to_owned(), "session".to_owned()));
        }));
        assert_eq!(read(&keys).as_deref(), Some("fresh"));
        assert_eq!(read(&keys).as_deref(), Some("fresh"));

        let keys = Memory::with(&[(OLD, "session", "stale")]);
        *keys.meanwhile.borrow_mut() = Some(Box::new(|keys: &Memory| {
            keys.entries.borrow_mut().clear();
        }));
        assert_eq!(read(&keys), None);
        assert!(!keys.has(NEW, "session"));
    }

    /// 锁拿不到（数据目录还没有）：值照样给，但不搬，等下次。
    #[test]
    fn adopting_waits_for_next_time_when_the_lock_is_unavailable() {
        let mut keys = Memory::with(&[(OLD, "session", "refresh-1")]);
        keys.locked = true;
        assert_eq!(read(&keys).as_deref(), Some("refresh-1"));
        assert!(keys.has(OLD, "session") && !keys.has(NEW, "session"));
        keys.locked = false;
        assert_eq!(read(&keys).as_deref(), Some("refresh-1"));
        assert!(!keys.has(OLD, "session") && keys.has(NEW, "session"));
    }

    #[test]
    fn signing_in_or_out_never_lets_a_stale_old_login_come_back() {
        let keys = Memory::with(&[(OLD, "session", "stale")]);
        assert!(write_secret_in(&keys, NEW, "session", "fresh").is_ok());
        assert!(!keys.has(OLD, "session"));
        assert!(delete_secret_in(&keys, NEW, "session").is_ok());
        assert_eq!(read(&keys), None);

        let keys = Memory::with(&[(OLD, "session", "stale")]);
        assert!(delete_secret_in(&keys, NEW, "session").is_ok());
        assert_eq!(read(&keys), None);
    }

    #[test]
    fn only_our_own_services_have_an_old_name() {
        assert_eq!(old_service(NEW).as_deref(), Some(OLD));
        assert_eq!(old_service("Claude Code-credentials"), None);
    }

    #[test]
    fn the_old_data_directory_is_renamed_once_and_never_split_in_two() {
        let home =
            crate::tool_adapters::common::temporary_working_directory("legacy-home").unwrap();
        let home = home.as_path();
        // 全新安装：直接用新名字。
        assert_eq!(data_root(home), home.join(HOME));
        std::fs::create_dir_all(home.join(OLD_HOME).join("tools")).unwrap();
        std::fs::write(home.join(OLD_HOME).join("device-id"), "abc").unwrap();
        // 还没搬：继续用旧目录。
        assert_eq!(data_root(home), home.join(OLD_HOME));
        assert!(adopt_directory(&home.join(OLD_HOME), &home.join(HOME)));
        assert_eq!(data_root(home), home.join(HOME));
        assert_eq!(
            std::fs::read_to_string(home.join(HOME).join("device-id")).unwrap(),
            "abc"
        );
        assert!(!home.join(OLD_HOME).exists());
        // 旧版又被打开、重新建了旧目录：不再搬，也不覆盖新目录。
        std::fs::create_dir_all(home.join(OLD_HOME)).unwrap();
        assert!(!adopt_directory(&home.join(OLD_HOME), &home.join(HOME)));
        assert_eq!(data_root(home), home.join(HOME));
    }

    #[test]
    fn the_data_directory_stays_put_while_an_old_client_is_mid_operation() {
        let home =
            crate::tool_adapters::common::temporary_working_directory("legacy-busy").unwrap();
        let (old, current) = (home.as_path().join(OLD_HOME), home.as_path().join(HOME));
        let recovery = old.join("connection-recovery");
        std::fs::create_dir_all(&recovery).unwrap();
        std::fs::write(recovery.join("claude_code.journal"), "pending").unwrap();
        // 旧版客户端做到一半：锁在它手里。
        let busy = crate::connection_recovery::operation_lock_at(&recovery).unwrap();
        assert!(!adopt_home(&old, &current));
        assert!(recovery.join("claude_code.journal").exists());
        assert_eq!(data_root(home.as_path()), old);
        // 它做完了：这次搬走，记录跟着过来。
        drop(busy);
        assert!(adopt_home(&old, &current));
        assert_eq!(
            std::fs::read_to_string(current.join("connection-recovery/claude_code.journal"))
                .unwrap(),
            "pending"
        );
        assert!(!old.exists());
    }

    fn quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }

    #[test]
    fn a_helper_command_follows_the_renamed_client() {
        let current = Path::new("/Applications/开饭.app/Contents/MacOS/kaifan-desktop");
        let old = "'/Applications/野菜API.app/Contents/MacOS/yeschoy-desktop'";
        assert_eq!(
            followed_command(
                &format!("{old} gateway-credential-helper claude_code"),
                "claude_code",
                current,
                quote
            )
            .as_deref(),
            Some("'/Applications/开饭.app/Contents/MacOS/kaifan-desktop' gateway-credential-helper claude_code")
        );
        assert_eq!(
            followed_command(&format!("{old} credential-helper pi"), "pi", current, quote)
                .as_deref(),
            Some("'/Applications/开饭.app/Contents/MacOS/kaifan-desktop' credential-helper pi")
        );
        // Windows 的写法。
        assert!(followed_command(
            "\"C:\\Users\\a\\AppData\\Local\\野菜API\\yeschoy-desktop.exe\" credential-helper pi",
            "pi",
            current,
            quote
        )
        .is_some());
    }

    #[test]
    fn a_helper_command_is_left_alone_when_it_is_current_foreign_or_still_valid() {
        let home =
            crate::tool_adapters::common::temporary_working_directory("legacy-helper").unwrap();
        let other = home.join("kaifan-desktop");
        std::fs::write(&other, b"").unwrap();
        let current = Path::new("/opt/kaifan/kaifan-desktop");
        let command = |path: &str| format!("{} credential-helper pi", quote(path));
        // 已经是现在的位置。
        assert_eq!(
            followed_command(&command("/opt/kaifan/kaifan-desktop"), "pi", current, quote),
            None
        );
        // 另一份还在的客户端（比如同时装了两份）：不抢。
        assert_eq!(
            followed_command(&command(other.to_str().unwrap()), "pi", current, quote),
            None
        );
        // 不是我们的命令、是别的应用的命令。
        assert_eq!(
            followed_command("op read op://x/y", "pi", current, quote),
            None
        );
        assert_eq!(
            followed_command(
                &command("/gone/kaifan-desktop"),
                "claude_code",
                current,
                quote
            ),
            None
        );
        // 挪了位置、原处已经没有了：跟过去。
        assert!(followed_command(&command("/gone/kaifan-desktop"), "pi", current, quote).is_some());
    }
}
