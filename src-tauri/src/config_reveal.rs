//! Reveal a tool's configuration folder in the system file manager.
//! PRD 6.8 高级模式 read-only support; the frontend localizes failures.
use std::path::{Path, PathBuf};
use std::process::Command;

use tauri::AppHandle;

use crate::tool_adapters::{self, claude_desktop};

fn config_dir_for(app: &str, home: &Path) -> Option<PathBuf> {
    match app {
        "claude" => Some(home.join(".claude")),
        "codex" => Some(home.join(".codex")),
        "pi" => Some(home.join(".pi").join("agent")),
        "opencode" => Some(crate::tool_adapters::opencode::config_dir(home)),
        // The Claude Desktop profile lives in the 3p config library
        // (`Claude-3p/configLibrary`); reveal that directory.
        "claude-desktop" => {
            let (_, _, profile_path, _) = claude_desktop::current_paths(home).ok()?;
            profile_path.parent().map(|dir| dir.to_path_buf())
        }
        _ => None,
    }
}

/// Resolve a directory that already exists.
///
/// Revealing must not create it: a tool the user never configured has no folder
/// worth showing, and creating one here would be the only place the assistant
/// writes to disk before the user confirms an activation.
fn reveal_target(app: &str, home: &Path) -> Result<PathBuf, &'static str> {
    let dir = config_dir_for(app, home).ok_or("unsupported_app")?;
    if !dir.is_dir() {
        return Err("config_folder_missing");
    }
    Ok(dir)
}

fn reveal_command(path: &Path) -> Command {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("/usr/bin/open");
    #[cfg(target_os = "windows")]
    let mut command = Command::new("explorer");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = Command::new("xdg-open");
    command.arg(path);
    command
}

#[tauri::command]
pub async fn open_config_folder(_handle: AppHandle, app: String) -> Result<bool, &'static str> {
    let home = tool_adapters::user_home().ok_or("config_folder_unavailable")?;
    let config_dir = reveal_target(&app, &home)?;
    let mut command = reveal_command(&config_dir);
    tauri::async_runtime::spawn_blocking(move || command.status())
        .await
        .map_err(|_| "reveal_failed")?
        .map_err(|_| "reveal_failed")?
        .success()
        .then_some(true)
        .ok_or("reveal_failed")
}

#[cfg(test)]
mod tests {
    use super::{config_dir_for, reveal_target};
    use std::path::Path;

    #[test]
    fn maps_supported_tools_to_config_dirs() {
        let home = Path::new("/home/tester");
        assert_eq!(
            config_dir_for("claude", home),
            Some(Path::new("/home/tester/.claude").to_path_buf())
        );
        assert_eq!(
            config_dir_for("codex", home),
            Some(Path::new("/home/tester/.codex").to_path_buf())
        );
        assert_eq!(
            config_dir_for("pi", home),
            Some(Path::new("/home/tester/.pi/agent").to_path_buf())
        );
        #[cfg(target_os = "macos")]
        assert_eq!(
            config_dir_for("claude-desktop", home),
            Some(
                Path::new("/home/tester/Library/Application Support/Claude-3p/configLibrary")
                    .to_path_buf()
            )
        );
    }

    #[test]
    fn rejects_unsupported_apps() {
        let home = Path::new("/home/tester");
        assert!(config_dir_for("hermes", home).is_none());
        assert!(config_dir_for("openclaw", home).is_none());
        assert!(config_dir_for("dsh", home).is_none());
        assert!(config_dir_for("", home).is_none());
    }

    #[test]
    fn only_reveals_existing_directories_and_never_creates_them() {
        let root = std::env::temp_dir().join(format!(
            "yeschoy-reveal-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".claude")).unwrap();

        assert_eq!(reveal_target("claude", &root), Ok(root.join(".claude")));
        assert_eq!(reveal_target("codex", &root), Err("config_folder_missing"));
        assert_eq!(reveal_target("dsh", &root), Err("unsupported_app"));
        assert!(
            !root.join(".codex").exists(),
            "revealing must not create the folder"
        );

        std::fs::remove_dir_all(&root).ok();
    }
}
