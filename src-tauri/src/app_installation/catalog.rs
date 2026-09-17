use reqwest::Url;

#[derive(Clone, Copy, Debug)]
pub(super) struct Source {
    pub tool: &'static str,
    pub url: &'static str,
    pub extension: &'static str,
    pub identity: &'static str,
    pub publisher: &'static str,
    pub architecture: &'static str,
}

pub(super) fn platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        "linux" => "linux",
        _ => "unknown",
    }
}

pub(super) fn architecture() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        // Detect Apple Silicon even when this assistant runs under Rosetta.
        unsafe extern "C" {
            fn sysctlbyname(
                name: *const std::ffi::c_char,
                old: *mut std::ffi::c_void,
                len: *mut usize,
                new: *mut std::ffi::c_void,
                new_len: usize,
            ) -> i32;
        }
        let mut arm: i32 = 0;
        let mut len = std::mem::size_of_val(&arm);
        // SAFETY: fixed NUL-terminated name and correctly sized output buffer.
        let result = unsafe {
            sysctlbyname(
                c"hw.optional.arm64".as_ptr(),
                (&mut arm as *mut i32).cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if result == 0 && arm == 1 {
            return "arm64";
        }
    }
    #[cfg(target_os = "windows")]
    if ["PROCESSOR_ARCHITEW6432", "PROCESSOR_ARCHITECTURE"]
        .iter()
        .any(|key| std::env::var(key).is_ok_and(|value| value.eq_ignore_ascii_case("ARM64")))
    {
        return "arm64";
    }
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        _ => "unknown",
    }
}

pub(super) const WORKBUDDY_HISTORY: &str =
    "https://www.workbuddy.cn/docs/workbuddy/Download-History";

pub(super) fn workbuddy_channel(source: Source) -> Option<&'static str> {
    match (source.tool, source.extension, source.architecture) {
        ("workbuddy", "dmg", "arm64") => Some("darwin-arm64"),
        ("workbuddy", "dmg", "x64") => Some("darwin-x64"),
        ("workbuddy", "exe", _) => Some("win32-x64-user"),
        _ => None,
    }
}

pub(super) fn source(tool: &str, os: &str, arch: &str) -> Option<Source> {
    let (url, extension, identity, publisher, architecture) = match (tool, os, arch) {
        ("codex_desktop", "macos", "arm64") => (
            "https://persistent.oaistatic.com/codex-app-prod/Codex.dmg",
            "dmg",
            "com.openai.codex",
            "2DC432GLL2",
            "arm64",
        ),
        ("claude_desktop", "macos", "arm64" | "x64") => (
            "https://downloads.claude.ai/releases/darwin/universal/RELEASES.json",
            "zip",
            "com.anthropic.claudefordesktop",
            "Q6L2SF6YDW",
            if arch == "arm64" { "arm64" } else { "x64" },
        ),
        ("codex_desktop", "windows", "x64") => (
            "https://persistent.oaistatic.com/codex-app-prod/ChatGPT-x64.msix",
            "msix",
            "OpenAI.Codex",
            "CN=50BDFD77-8903-4850-9FFE-6E8522F64D5B",
            "x64",
        ),
        ("codex_desktop", "windows", "arm64") => (
            "https://persistent.oaistatic.com/codex-app-prod/ChatGPT-arm64.msix",
            "msix",
            "OpenAI.Codex",
            "CN=50BDFD77-8903-4850-9FFE-6E8522F64D5B",
            "arm64",
        ),
        ("claude_desktop", "windows", "x64") => (
            "https://claude.ai/api/desktop/win32/x64/msix/latest/redirect",
            "msix",
            "Claude",
            "CN=\"Anthropic, PBC\", O=\"Anthropic, PBC\", L=San Francisco, S=California, C=US, SERIALNUMBER=4860621, OID.2.5.4.15=Private Organization, OID.1.3.6.1.4.1.311.60.2.1.2=Delaware, OID.1.3.6.1.4.1.311.60.2.1.3=US",
            "x64",
        ),
        ("claude_desktop", "windows", "arm64") => (
            "https://claude.ai/api/desktop/win32/arm64/msix/latest/redirect",
            "msix",
            "Claude",
            "CN=\"Anthropic, PBC\", O=\"Anthropic, PBC\", L=San Francisco, S=California, C=US, SERIALNUMBER=4860621, OID.2.5.4.15=Private Organization, OID.1.3.6.1.4.1.311.60.2.1.2=Delaware, OID.1.3.6.1.4.1.311.60.2.1.3=US",
            "arm64",
        ),
        ("workbuddy", "macos", "arm64") => (
            WORKBUDDY_HISTORY,
            "dmg",
            "com.tencent.workbuddy.mac",
            "FN2V63AD2J",
            "arm64",
        ),
        ("workbuddy", "macos", "x64") => (
            WORKBUDDY_HISTORY,
            "dmg",
            "com.tencent.workbuddy.mac",
            "FN2V63AD2J",
            "x64",
        ),
        ("workbuddy", "windows", "x64" | "arm64") => {
            (WORKBUDDY_HISTORY, "exe", "WorkBuddy", "Tencent", "x64")
        }
        _ => return None,
    };
    Some(Source {
        tool: match tool {
            "codex_desktop" => "codex_desktop",
            "workbuddy" => "workbuddy",
            _ => "claude_desktop",
        },
        url,
        extension,
        identity,
        publisher,
        architecture,
    })
}

pub(super) fn mode(tool: &str, os: &str, arch: &str) -> &'static str {
    if source(tool, os, arch).is_some() {
        if os == "windows" {
            "system_assisted"
        } else {
            "automatic"
        }
    } else if matches!(tool, "codex_desktop" | "claude_desktop" | "workbuddy") {
        "unsupported"
    } else {
        "guided"
    }
}

pub(super) fn guide(tool: &str) -> Option<&'static str> {
    Some(match tool {
        "codex_desktop" => "https://learn.chatgpt.com/docs/app",
        "claude_desktop" => "https://claude.com/download",
        "claude_code" => "https://code.claude.com/docs/en/setup",
        "pi" => "https://github.com/earendil-works/pi/tree/main/packages/coding-agent",
        "dsh_web" => "https://github.com/deepseek-ai/deepseek-harness",
        "workbuddy" => "https://www.workbuddy.cn/",
        _ => return None,
    })
}

pub(super) fn allowed_url(source: Source, url: &Url) -> bool {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
        || url.port_or_known_default() != Some(443)
        || url.path().contains('%')
        || url.path().contains('\\')
    {
        return false;
    }
    match (source.tool, url.host_str()) {
        ("codex_desktop", Some("persistent.oaistatic.com")) => {
            url.path().starts_with("/codex-app-prod/")
                && url.path().ends_with(&format!(".{}", source.extension))
        }
        ("claude_desktop", Some("claude.ai")) => url.as_str() == source.url,
        ("claude_desktop", Some("downloads.claude.ai")) => {
            let prefix = if source.extension == "zip" {
                "/releases/darwin/universal/".to_string()
            } else {
                format!("/releases/win32/{}/", source.architecture)
            };
            url.path().starts_with(&prefix)
                && url.path().ends_with(&format!(".{}", source.extension))
        }
        ("workbuddy", Some("download.codebuddy.cn")) => {
            let Some(channel) = workbuddy_channel(source) else {
                return false;
            };
            let prefix = format!("/workbuddy/saas/{channel}/WorkBuddy-{channel}-");
            let suffix = format!(".{}", source.extension);
            let path = url.path();
            path.starts_with(&prefix)
                && path.ends_with(&suffix)
                && path[prefix.len()..path.len() - suffix.len()]
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        }
        _ => false,
    }
}

pub(super) fn source_id(source: Source) -> String {
    let app = match source.tool {
        "codex_desktop" => "codex",
        "workbuddy" => "workbuddy",
        _ => "claude",
    };
    let os = if matches!(source.extension, "msix" | "exe") {
        "windows"
    } else {
        "macos"
    };
    let arch = if source.extension == "zip" {
        "universal"
    } else {
        source.architecture
    };
    format!("{app}-{os}-{arch}")
}
