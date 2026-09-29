//! Where a Windows command-line tool can be when this process's `PATH` does not
//! say so.
//!
//! A Windows GUI process gets its environment once, when Explorer starts it.
//! Three real installations fall through that:
//!
//!   * A tool installed, or a `PATH` entry added, after the assistant started.
//!     The user's registry `PATH` has it and a new console sees it, but this
//!     process keeps the old copy until it restarts, so "重新检查" kept saying
//!     "未发现应用".
//!   * npm with a global `prefix` on another drive (`npm config set prefix
//!     D:\...`). The shims live in that prefix, which is often not on `PATH`.
//!   * DeepSeek Harness Desktop. Its `dsh.cmd` ships inside the installation
//!     (`resources\runtime\cli\bin`), and putting it on `PATH` is an optional
//!     menu action, so the common case is a working CLI on no `PATH` at all,
//!     in whatever folder the installer was pointed at.
//!
//! Everything here only *adds* directories to search. Whether a file in them is
//! the tool is still decided by the discovery code that searches them.

#[cfg(any(target_os = "windows", test))]
use std::path::{Path, PathBuf};

/// Largest npmrc read. A real one is a few lines; this bounds a hostile file.
#[cfg(target_os = "windows")]
const NPMRC_LIMIT: u64 = 64 * 1024;

/// Upper bound on directories taken from one registry `PATH` value.
#[cfg(any(target_os = "windows", test))]
const MAX_REGISTRY_PATH_ENTRIES: usize = 256;

/// Where Harness Desktop keeps its CLI, relative to the installation folder.
#[cfg(any(target_os = "windows", test))]
const DSH_DESKTOP_CLI: [&str; 4] = ["resources", "runtime", "cli", "bin"];

/// Expand `%NAME%` the way Windows expands a `REG_EXPAND_SZ` value.
///
/// An unknown name stays as written, which is also what Windows does; the
/// directory it produces then simply does not exist.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn expand_percent_variables(
    raw: &str,
    lookup: impl Fn(&str) -> Option<String>,
) -> String {
    let mut expanded = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find('%') {
        expanded.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => {
                let name = &after[..end];
                match lookup(name) {
                    Some(value) => expanded.push_str(&value),
                    None => {
                        expanded.push('%');
                        expanded.push_str(name);
                        expanded.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                expanded.push('%');
                rest = after;
            }
        }
    }
    expanded.push_str(rest);
    expanded
}

/// Split a `PATH`-style list. Quotes around an entry are a common hand-edit and
/// cmd.exe tolerates them, so they are removed; relative entries are dropped,
/// because they would resolve against whatever directory this process is in.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn split_path_list(raw: &str) -> Vec<PathBuf> {
    raw.split(';')
        .map(|entry| entry.trim().trim_matches('"').trim())
        .filter(|entry| !entry.is_empty())
        .filter(|entry| is_absolute_windows_path(entry))
        .take(MAX_REGISTRY_PATH_ENTRIES)
        .map(PathBuf::from)
        .collect()
}

/// `C:\...` or `\\server\share\...`. Checked on the string so the rule is the
/// same on the Linux and macOS machines that run the unit tests.
#[cfg(any(target_os = "windows", test))]
fn is_absolute_windows_path(entry: &str) -> bool {
    let bytes = entry.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    drive || entry.starts_with("\\\\")
}

/// The `prefix` an npmrc sets, if any.
///
/// npm reads ini: `;` and `#` start a comment, keys are case-sensitive, and
/// `${NAME}` is an environment reference. A double-quoted value is JSON, which
/// matters because that is how a Windows path with backslashes gets quoted.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn npmrc_prefix(
    contents: &str,
    lookup: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    let mut prefix = None;
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "prefix" {
            continue;
        }
        let value = value.trim();
        let value = if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
            serde_json::from_str::<String>(value).ok()?
        } else if value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'') {
            value[1..value.len() - 1].to_owned()
        } else {
            value.to_owned()
        };
        // Later lines win, as in npm.
        prefix = Some(expand_dollar_braces(&value, &lookup));
    }
    prefix.filter(|value| is_absolute_windows_path(value))
}

#[cfg(any(target_os = "windows", test))]
fn expand_dollar_braces(raw: &str, lookup: &impl Fn(&str) -> Option<String>) -> String {
    let mut expanded = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find("${") {
        expanded.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            expanded.push_str(&rest[start..]);
            return expanded;
        };
        let name = &after[..end];
        match lookup(name) {
            Some(value) => expanded.push_str(&value),
            None => {
                expanded.push_str("${");
                expanded.push_str(name);
                expanded.push('}');
            }
        }
        rest = &after[end + 1..];
    }
    expanded.push_str(rest);
    expanded
}

/// Drop trailing version tokens from an uninstall `DisplayName`.
///
/// electron-builder's NSIS installer registers `"<Product> <version>"` by
/// default, so an exact name comparison missed every such installation.
/// A token counts as a version when it starts with a digit, or with `v`
/// followed by a digit (`1.2.3`, `0.2.0-rc.2`, `v2`, `(1.4.0)`).
#[cfg(any(target_os = "windows", test))]
pub(crate) fn without_version_suffix(display_name: &str) -> &str {
    let mut name = display_name.trim_end();
    while let Some((head, last)) = name.rsplit_once(char::is_whitespace) {
        let token = last.trim_start_matches(['(', '[']);
        let token = token.strip_prefix(['v', 'V']).unwrap_or(token);
        if !token.starts_with(|character: char| character.is_ascii_digit()) {
            break;
        }
        name = head.trim_end();
    }
    name
}

/// Whether an uninstall entry is DeepSeek Harness Desktop.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn is_dsh_desktop_display_name(display_name: &str) -> bool {
    let name = without_version_suffix(display_name)
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect::<String>();
    name == "deepseekharness"
}

/// The CLI folder of a Harness Desktop installation.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn dsh_desktop_cli_directory(installation: &Path) -> PathBuf {
    DSH_DESKTOP_CLI
        .iter()
        .fold(installation.to_path_buf(), |path, part| path.join(part))
}

/// The installation folder an uninstall entry points at: `InstallLocation`
/// when set, otherwise the folder of the `DisplayIcon` executable.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn installation_folder(install_location: &str, display_icon: &str) -> Option<PathBuf> {
    let location = install_location.trim().trim_matches('"').trim();
    if !location.is_empty() && is_absolute_windows_path(location) {
        return Some(PathBuf::from(location));
    }
    let icon = display_icon.trim();
    let icon = match icon.strip_prefix('"') {
        Some(rest) => rest.split_once('"').map(|(path, _)| path)?,
        None => match icon.rsplit_once(',') {
            Some((path, index)) if index.trim().parse::<i32>().is_ok() => path,
            _ => icon,
        },
    }
    .trim();
    if !icon.to_ascii_lowercase().ends_with(".exe") || !is_absolute_windows_path(icon) {
        return None;
    }
    // Split on both separators so the fixture behaves the same off Windows.
    let (folder, _) = icon.rsplit_once(['\\', '/'])?;
    Some(PathBuf::from(folder))
}

/// Directories on the user's and the machine's registry `PATH`, expanded.
///
/// This is what a console opened *now* would search. It is read on every call
/// on purpose: the whole point is to see changes made after this process began.
#[cfg(target_os = "windows")]
pub(crate) fn registry_path_directories() -> Vec<PathBuf> {
    use winreg::{enums::KEY_READ, RegKey, HKCU, HKLM};

    let user = HKCU.open_subkey_with_flags("Environment", KEY_READ).ok();
    let machine = HKLM
        .open_subkey_with_flags(
            "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
            KEY_READ,
        )
        .ok();
    let read = |key: &Option<RegKey>, name: &str| -> Option<String> {
        key.as_ref()?.get_value::<String, _>(name).ok()
    };
    // The registry first: a variable this process inherited may have been
    // changed since, by the same installer that changed `Path`, and one set
    // after launch is only there. User values shadow machine values, the order
    // Windows builds an environment in. Dynamic names Windows never stores
    // there (`USERPROFILE`, `SystemRoot`) fall through to this process's copy.
    let lookup = |name: &str| -> Option<String> {
        read(&user, name)
            .or_else(|| read(&machine, name))
            .or_else(|| std::env::var(name).ok())
    };
    let mut directories = Vec::new();
    // Windows puts the machine PATH first and appends the user's.
    for key in [&machine, &user] {
        if let Some(raw) = read(key, "Path") {
            directories.extend(split_path_list(&expand_percent_variables(&raw, lookup)));
        }
    }
    directories
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn registry_path_directories() -> Vec<std::path::PathBuf> {
    Vec::new()
}

/// npm global prefixes configured in the user's npmrc files.
#[cfg(target_os = "windows")]
pub(crate) fn npm_prefix_directories() -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Some(home) = crate::tool_adapters::user_home() {
        files.push(home.join(".npmrc"));
    }
    if let Some(app_data) = std::env::var_os("APPDATA") {
        files.push(
            PathBuf::from(app_data)
                .join("npm")
                .join("etc")
                .join("npmrc"),
        );
    }
    let lookup = |name: &str| std::env::var(name).ok();
    let mut directories = Vec::new();
    for file in files {
        let Some(contents) = read_bounded_text(&file, NPMRC_LIMIT) else {
            continue;
        };
        if let Some(prefix) = npmrc_prefix(&contents, lookup) {
            // On Windows npm writes the global shims into the prefix itself,
            // not into a `bin` below it.
            let prefix = PathBuf::from(prefix);
            if !directories.contains(&prefix) {
                directories.push(prefix);
            }
        }
    }
    directories
}

#[cfg(target_os = "windows")]
fn read_bounded_text(path: &Path, maximum: u64) -> Option<String> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > maximum {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// CLI folders of every Harness Desktop installation Windows knows about, on
/// any drive, plus the installer's default location.
#[cfg(target_os = "windows")]
pub(crate) fn dsh_desktop_cli_directories() -> Vec<PathBuf> {
    use winreg::{
        enums::{KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY},
        HKCU, HKLM,
    };
    const UNINSTALL: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";

    let mut directories = Vec::new();
    let mut add = |directory: PathBuf| {
        if !directories.contains(&directory) {
            directories.push(directory);
        }
    };
    // The installer is per-user, so HKCU is where it registers. HKLM is read
    // too in case a repackaged or future per-machine build turns up.
    for hive in [&HKCU, &HKLM] {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            let Ok(parent) = hive.open_subkey_with_flags(UNINSTALL, KEY_READ | view) else {
                continue;
            };
            for name in parent.enum_keys().filter_map(Result::ok).take(4096) {
                let Ok(item) = parent.open_subkey_with_flags(&name, KEY_READ | view) else {
                    continue;
                };
                let display_name: String = item.get_value("DisplayName").unwrap_or_default();
                if !is_dsh_desktop_display_name(&display_name) {
                    continue;
                }
                let install_location: String =
                    item.get_value("InstallLocation").unwrap_or_default();
                let display_icon: String = item.get_value("DisplayIcon").unwrap_or_default();
                if let Some(folder) = installation_folder(&install_location, &display_icon) {
                    add(dsh_desktop_cli_directory(&folder));
                }
            }
        }
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        add(dsh_desktop_cli_directory(
            &PathBuf::from(local_app_data)
                .join("Programs")
                .join("DeepSeek Harness"),
        ));
    }
    directories
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment(name: &str) -> Option<String> {
        match name {
            "USERPROFILE" => Some("C:\\Users\\me".into()),
            "LOCALAPPDATA" => Some("C:\\Users\\me\\AppData\\Local".into()),
            _ => None,
        }
    }

    #[test]
    fn registry_path_expands_known_names_and_keeps_unknown_ones() {
        assert_eq!(
            expand_percent_variables("%USERPROFILE%\\bin;%NOPE%\\x;50%;%", environment),
            "C:\\Users\\me\\bin;%NOPE%\\x;50%;%"
        );
        assert_eq!(expand_percent_variables("%%", environment), "%%");
    }

    #[test]
    fn path_list_drops_blank_relative_and_quoted_noise() {
        let entries = split_path_list(
            " D:\\nodejs\\ ;;\"D:\\Tools\\bin\";relative\\dir;\\\\server\\share\\bin;C:/Windows;",
        );
        assert_eq!(
            entries,
            vec![
                PathBuf::from("D:\\nodejs\\"),
                PathBuf::from("D:\\Tools\\bin"),
                PathBuf::from("\\\\server\\share\\bin"),
                PathBuf::from("C:/Windows"),
            ]
        );
    }

    #[test]
    fn npmrc_prefix_reads_plain_quoted_and_referenced_values() {
        assert_eq!(
            npmrc_prefix(
                "; comment\nregistry=https://r\nprefix=D:\\npm-global\n",
                environment
            ),
            Some("D:\\npm-global".into())
        );
        assert_eq!(
            npmrc_prefix("prefix = \"D:\\\\npm global\"\n", environment),
            Some("D:\\npm global".into())
        );
        assert_eq!(
            npmrc_prefix("prefix=${USERPROFILE}\\npm\n", environment),
            Some("C:\\Users\\me\\npm".into())
        );
        // The last assignment wins, as in npm.
        assert_eq!(
            npmrc_prefix("prefix=C:\\a\nprefix=D:\\b\n", environment),
            Some("D:\\b".into())
        );
        // Commented out, relative, unresolved or unrelated keys are not a prefix.
        assert_eq!(npmrc_prefix("# prefix=D:\\x\n", environment), None);
        assert_eq!(npmrc_prefix("prefix=npm\n", environment), None);
        assert_eq!(npmrc_prefix("prefix=${NOPE}\\npm\n", environment), None);
        assert_eq!(npmrc_prefix("cache-prefix=D:\\x\n", environment), None);
    }

    #[test]
    fn version_suffixes_are_removed_from_display_names() {
        assert_eq!(without_version_suffix("WorkBuddy 1.2.3"), "WorkBuddy");
        assert_eq!(
            without_version_suffix("DeepSeek Harness 0.2.0-rc.2"),
            "DeepSeek Harness"
        );
        assert_eq!(without_version_suffix("Claude v2 (1.4.0)"), "Claude");
        assert_eq!(without_version_suffix("WorkBuddy"), "WorkBuddy");
        // A name that is only a number is left alone rather than emptied.
        assert_eq!(without_version_suffix("7"), "7");
        assert_eq!(without_version_suffix("Tool Pro"), "Tool Pro");
    }

    #[test]
    fn harness_desktop_is_recognised_only_by_its_own_name() {
        assert!(is_dsh_desktop_display_name("DeepSeek Harness"));
        assert!(is_dsh_desktop_display_name("DeepSeek Harness 0.2.0"));
        assert!(is_dsh_desktop_display_name("deepseek-harness 0.2.0-rc.2"));
        assert!(!is_dsh_desktop_display_name("DeepSeek Harness Helper"));
        assert!(!is_dsh_desktop_display_name("DeepSeek"));
        assert!(!is_dsh_desktop_display_name("DSH Desktop"));
    }

    #[test]
    fn installation_folder_prefers_install_location_then_icon() {
        assert_eq!(
            installation_folder("D:\\Apps\\DeepSeek Harness", "C:\\elsewhere\\x.exe,0"),
            Some(PathBuf::from("D:\\Apps\\DeepSeek Harness"))
        );
        assert_eq!(
            installation_folder("", "\"D:\\Apps\\DeepSeek Harness\\DeepSeek Harness.exe\",0"),
            Some(PathBuf::from("D:\\Apps\\DeepSeek Harness"))
        );
        assert_eq!(
            installation_folder("", "D:\\Apps\\DSH\\DeepSeek Harness.exe,0"),
            Some(PathBuf::from("D:\\Apps\\DSH"))
        );
        assert_eq!(installation_folder("relative", "icon.ico"), None);
        assert_eq!(installation_folder("", ""), None);
    }

    #[test]
    fn cli_directory_follows_the_desktop_layout() {
        let folder = dsh_desktop_cli_directory(Path::new("D:/Apps/DeepSeek Harness"));
        assert!(folder.ends_with(Path::new("resources/runtime/cli/bin")));
        assert!(folder.starts_with("D:/Apps/DeepSeek Harness"));
    }
}
