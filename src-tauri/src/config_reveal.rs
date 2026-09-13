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
        // The Claude Desktop profile lives in the 3p config library
        // (`Claude-3p/configLibrary`); reveal that directory.
        "claude-desktop" => {
            let (_, _, profile_path, _) = claude_desktop::current_paths(home).ok()?;
            profile_path.parent().map(|dir| dir.to_path_buf())
        }
        _ => None,
    }
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
pub async fn open_config_folder(_handle: AppHandle, app: String) -> Result<bool, String> {
    let home = tool_adapters::user_home()
        .ok_or_else(|| "user home unavailable".to_string())?;
    let config_dir = config_dir_for(&app, &home)
        .ok_or_else(|| format!("unsupported app: {app}"))?;
    std::fs::create_dir_all(&config_dir)
        .map_err(|error| format!("create config dir failed: {error}"))?;
    let mut command = reveal_command(&config_dir);
    tauri::async_runtime::spawn_blocking(move || command.status())
        .await
        .map_err(|error| format!("reveal spawn failed: {error}"))?
        .map_err(|error| format!("reveal failed: {error}"))?
        .success()
        .then_some(true)
        .ok_or_else(|| "reveal failed".to_string())
}

#[cfg(test)]
mod tests {
    use super::config_dir_for;
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
                Path::new(
                    "/home/tester/Library/Application Support/Claude-3p/configLibrary"
                )
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
}
