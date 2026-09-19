//! The environment a command-line tool actually starts with.
//!
//! A macOS app launched from Finder, the Dock or Launchpad inherits launchd's
//! environment. It never sources `.zprofile` or `.zshrc`. But
//! `tool_adapters::terminal_launch` hands the command to Terminal.app, which
//! runs the user's *login* shell and does source them. So the process that
//! decides how to configure a tool and the process that later runs that tool
//! see two different environments.
//!
//! That gap produced two separate classes of silent failure:
//!
//!   * Override guards (`ANTHROPIC_BASE_URL`, `CODEX_HOME`, `DSH_HOME`,
//!     `PI_CODING_AGENT_DIR`) never fired, because the exports live in the
//!     user's shell rc and this process cannot see them. Activation reported
//!     success while the tool kept using someone else's relay — the exact
//!     situation those guards exist to prevent.
//!   * `PATH` was launchd's short list, so a CLI installed under nvm, bun,
//!     volta or fnm was reported as "not installed" even though it runs fine
//!     in the user's terminal.
//!
//! Both are read from here instead. Windows has no equivalent split — a GUI
//! process there inherits the same user/system environment a new shell gets —
//! so resolution is a no-op and every accessor falls back to `std::env`.
//!
//! Resolution is deliberately *not* lazy-on-first-use: the adapters that need
//! it are synchronous, and blocking them on a shell that sources oh-my-zsh
//! would stall a runtime thread for seconds. `initialize` is awaited once
//! during setup; until it completes (or if it fails) every accessor falls back
//! to this process's environment, which is exactly the old behaviour. Warm, it
//! is strictly better; cold, it is never worse.

use std::{collections::BTreeMap, ffi::OsString, sync::OnceLock};

/// Variables worth asking the login shell about. Keeping this closed means the
/// probe prints a handful of known-shaped lines instead of a whole environment,
/// so shell banners, MOTD noise and values containing `=` cannot confuse it.
#[cfg(not(target_os = "windows"))]
const WANTED: [&str; 7] = [
    "PATH",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "CODEX_HOME",
    "DSH_HOME",
    "PI_CODING_AGENT_DIR",
];

/// The subset of [`WANTED`] the probe clears before spawning, so a development
/// build started from an already-configured terminal cannot read its own values
/// back and mistake them for the user's rc files. `PATH` is deliberately not in
/// this list: unsetting it would leave rc lines of the form
/// `export PATH="$PATH:/foo"` producing a leading empty entry, which means the
/// current directory.
#[cfg(not(target_os = "windows"))]
const CLEARED_BEFORE_PROBE: [&str; 6] = [
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "CODEX_HOME",
    "DSH_HOME",
    "PI_CODING_AGENT_DIR",
];

#[cfg(not(target_os = "windows"))]
const BEGIN: &str = "__YESCHOY_ENV_BEGIN__";
#[cfg(not(target_os = "windows"))]
const END: &str = "__YESCHOY_ENV_END__";

static RESOLVED: OnceLock<BTreeMap<String, OsString>> = OnceLock::new();

/// Resolve the login shell's environment once. Safe to call more than once;
/// later calls are ignored. Never panics and never reports failure: an
/// unresolved environment is a supported state, not an error.
pub(crate) async fn initialize() {
    if RESOLVED.get().is_some() {
        return;
    }
    let resolved = probe().await.unwrap_or_default();
    // A probe that came back empty still gets stored, so a slow or broken shell
    // is not re-run on every scan.
    let _ = RESOLVED.set(resolved);
}

/// The value a terminal-launched tool would see for `key`.
///
/// Prefers the login shell. Falls back to this process's environment while the
/// probe has not completed, when it failed, and on Windows.
pub(crate) fn var_os(key: &str) -> Option<OsString> {
    match RESOLVED.get().and_then(|values| values.get(key)) {
        Some(value) if !value.is_empty() => Some(value.clone()),
        // A resolved-but-empty value means the login shell genuinely does not
        // set it. Falling back to this process's value would resurrect the bug
        // in reverse (a dev build started from a terminal would see exports the
        // user's Terminal window will not), so an empty resolved value is
        // authoritative and stops here.
        Some(_) => None,
        None => std::env::var_os(key).filter(|value| !value.is_empty()),
    }
}

/// True when `key` is set non-empty in *either* environment.
///
/// Refuse-to-proceed guards use this rather than [`var_os`]: when the two
/// environments disagree, the safe reading is that an override might reach the
/// tool. Warning about an override that turns out to be inert costs the user a
/// message; missing a real one bills them on somebody else's relay.
pub(crate) fn is_set_anywhere(key: &str) -> bool {
    let resolved = RESOLVED
        .get()
        .and_then(|values| values.get(key))
        .is_some_and(|value| !value.is_empty());
    resolved || std::env::var_os(key).is_some_and(|value| !value.is_empty())
}

/// Directories to search for a CLI: the login shell's `PATH` followed by this
/// process's, de-duplicated, order preserved.
///
/// Both are searched because either can hold entries the other lacks — launchd
/// contributes `/usr/bin` on a minimal login shell, and the login shell
/// contributes every version manager. Searching the union can only find more
/// than today.
pub(crate) fn search_directories() -> Vec<std::path::PathBuf> {
    let mut seen = std::collections::HashSet::new();
    let mut directories = Vec::new();
    let sources = [
        RESOLVED
            .get()
            .and_then(|values| values.get("PATH"))
            .cloned()
            .filter(|value| !value.is_empty()),
        std::env::var_os("PATH"),
    ];
    for source in sources.into_iter().flatten() {
        for directory in std::env::split_paths(&source) {
            if directory.as_os_str().is_empty() {
                continue;
            }
            if seen.insert(directory.clone()) {
                directories.push(directory);
            }
        }
    }
    directories
}

#[cfg(target_os = "windows")]
async fn probe() -> Option<BTreeMap<String, OsString>> {
    // A Windows GUI process already carries the user and system environment a
    // freshly opened console would get. There is nothing to recover.
    None
}

#[cfg(not(target_os = "windows"))]
async fn probe() -> Option<BTreeMap<String, OsString>> {
    use std::{process::Stdio, time::Duration};
    use tokio::{process::Command, time::timeout};

    // Interactive (`-i`) as well as login (`-l`): plenty of people export these
    // from `.zshrc`/`.bashrc`, which a non-interactive shell never reads.
    // An interactive rc can also be slow (oh-my-zsh, lazy nvm) or print
    // banners, which is why this is bounded and sentinel-delimited.
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| OsString::from("/bin/sh"));
    let mut command = Command::new(&shell);
    command
        .arg("-lic")
        .arg(script())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    for key in CLEARED_BEFORE_PROBE {
        command.env_remove(key);
    }
    let output = timeout(Duration::from_secs(10), command.output())
        .await
        .ok()?
        .ok()?;
    // The exit status is deliberately ignored. An interactive rc that ends on a
    // failing command is common and says nothing about whether the block below
    // printed. The sentinels are the success condition.
    let values = parse(&String::from_utf8_lossy(&output.stdout));
    (!values.is_empty()).then_some(values)
}

#[cfg(not(target_os = "windows"))]
fn script() -> String {
    // Built from a fixed, uppercase-ASCII key list, so no quoting hazard and no
    // `eval`. `${KEY-}` keeps an unset variable from aborting a shell running
    // under `set -u` from the user's rc.
    let mut script = format!("printf '%s\\n' '{BEGIN}'\n");
    for key in WANTED {
        script.push_str(&format!("printf '{key}=%s\\n' \"${{{key}-}}\"\n"));
    }
    script.push_str(&format!("printf '%s\\n' '{END}'\n"));
    script
}

/// Read only what sits between the sentinels, so anything an rc file prints on
/// startup is discarded rather than parsed as a variable.
#[cfg(not(target_os = "windows"))]
fn parse(output: &str) -> BTreeMap<String, OsString> {
    let mut values = BTreeMap::new();
    let mut inside = false;
    for line in output.lines() {
        let line = line.trim_end_matches('\r');
        if line == BEGIN {
            // A banner that happens to contain the sentinel would open a second
            // block; restarting discards whatever the first one collected.
            inside = true;
            values.clear();
            continue;
        }
        if line == END {
            if inside {
                break;
            }
            continue;
        }
        if !inside {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if WANTED.contains(&key) {
            values.insert(key.to_owned(), OsString::from(value));
        }
    }
    values
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;

    #[test]
    fn reads_only_what_sits_between_the_sentinels() {
        let noisy = format!(
            "Welcome to your shell!\nMOTD=not-a-variable\n{BEGIN}\n\
             PATH=/opt/homebrew/bin:/usr/bin\nANTHROPIC_BASE_URL=https://old-relay.example\n\
             {END}\nPI_CODING_AGENT_DIR=/after/the/end\n"
        );
        let values = parse(&noisy);
        assert_eq!(
            values.get("PATH").unwrap(),
            &OsString::from("/opt/homebrew/bin:/usr/bin")
        );
        assert_eq!(
            values.get("ANTHROPIC_BASE_URL").unwrap(),
            &OsString::from("https://old-relay.example")
        );
        assert!(!values.contains_key("MOTD"));
        // Anything after the closing sentinel is a banner, not an export.
        assert!(!values.contains_key("PI_CODING_AGENT_DIR"));
    }

    #[test]
    fn an_unset_variable_reads_as_empty_rather_than_missing_the_block() {
        let output = format!("{BEGIN}\nPATH=/usr/bin\nCODEX_HOME=\nDSH_HOME=\n{END}\n");
        let values = parse(&output);
        assert_eq!(values.get("CODEX_HOME").unwrap(), &OsString::from(""));
        assert_eq!(values.get("PATH").unwrap(), &OsString::from("/usr/bin"));
    }

    #[test]
    fn values_containing_equals_signs_survive_intact() {
        let output = format!("{BEGIN}\nANTHROPIC_AUTH_TOKEN=sk-a=b=c\n{END}\n");
        assert_eq!(
            parse(&output).get("ANTHROPIC_AUTH_TOKEN").unwrap(),
            &OsString::from("sk-a=b=c")
        );
    }

    #[test]
    fn a_probe_that_never_reaches_the_block_yields_nothing() {
        assert!(parse("command not found\n").is_empty());
        assert!(parse("").is_empty());
    }

    #[test]
    fn the_script_quotes_every_wanted_key_and_never_evaluates() {
        let script = script();
        assert!(!script.contains("eval"));
        for key in WANTED {
            assert!(script.contains(&format!("\"${{{key}-}}\"")), "{key}");
        }
    }

    #[test]
    fn the_real_login_shell_probe_reports_a_usable_path() {
        // Runs the actual probe against this machine's shell. It asserts only
        // what must hold anywhere a POSIX shell exists; a sandbox without one
        // resolves to None and the test still passes.
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let Some(values) = runtime.block_on(probe()) else {
            return;
        };
        let path = values.get("PATH").cloned().unwrap_or_default();
        assert!(!path.is_empty(), "a login shell always sets PATH");
        // The probe strips these before spawning, so anything that comes back
        // was set by the user's rc files rather than inherited from this test.
        assert!(values.contains_key("ANTHROPIC_BASE_URL"));
    }
}
