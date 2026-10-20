//! Compile-time distribution identity. Never inferred from a login/session or IPC input.
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Registry {
    schema_version: u8,
    official: Channel,
    partner: Channel,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Channel {
    authorization_origin: String,
    pub(crate) endpoint: String,
    pub(crate) public_key: String,
    #[serde(skip)]
    pub(crate) variant: &'static str,
}

const DISTRIBUTION: &str = env!("YESCHOY_DISTRIBUTION");
const GITHUB_REPO: &str = env!("YESCHOY_GITHUB_REPO");
const COMMUNITY_PUBKEY: &str = env!("YESCHOY_COMMUNITY_UPDATER_PUBKEY");
const CHANNELS_JSON: &str = include_str!(concat!(env!("OUT_DIR"), "/update-channels.json"));

pub(crate) fn compiled_channel() -> Result<Channel, &'static str> {
    match DISTRIBUTION {
        "official" | "partner" => {
            channel_for_origin(CHANNELS_JSON, env!("YESCHOY_AUTHORIZATION_PAGE_ORIGIN"))
        }
        _ => community_channel(GITHUB_REPO, COMMUNITY_PUBKEY),
    }
}

/// 公开构建：从 GitHub Releases 的 `latest.json` 检查更新。
/// 没有在构建时提供更新公钥时，更新功能整体不可用（而不是不验签）。
fn community_channel(repo: &str, public_key: &str) -> Result<Channel, &'static str> {
    let valid_repo = repo.split('/').count() == 2
        && repo
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.'));
    if !valid_repo {
        return Err("channel_invalid");
    }
    if public_key.is_empty() {
        return Err("updater_not_configured");
    }
    Ok(Channel {
        authorization_origin: String::new(),
        endpoint: format!("https://github.com/{repo}/releases/latest/download/latest.json"),
        public_key: public_key.to_owned(),
        variant: "community",
    })
}

fn channel_for_origin(json: &str, origin: &str) -> Result<Channel, &'static str> {
    let registry: Registry = serde_json::from_str(json).map_err(|_| "channel_invalid")?;
    if registry.schema_version != 1 || registry.official.public_key == registry.partner.public_key {
        return Err("channel_invalid");
    }
    let (mut channel, variant) = match origin {
        "https://yeschoy.com" => (registry.official, "official"),
        "https://ai.yeschoy.io" => (registry.partner, "partner"),
        _ => return Err("channel_invalid"),
    };
    if channel.authorization_origin != origin
        || channel.public_key.is_empty()
        || !channel.endpoint.starts_with("https://")
        || !channel
            .endpoint
            .ends_with(&format!("/updates/{variant}/stable.json"))
    {
        return Err("channel_invalid");
    }
    channel.variant = variant;
    Ok(channel)
}

impl Channel {
    pub(crate) fn accepts(&self, manifest: &serde_json::Value, url: &str, version: &str) -> bool {
        let filename = if cfg!(target_os = "windows") {
            format!(
                "yeschoy-{version}-{}-windows-x86_64-installer.exe",
                self.variant
            )
        } else {
            format!(
                "yeschoy-{version}-{}-macos-universal.app.tar.gz",
                self.variant
            )
        };
        let expected_url = if self.variant == "community" {
            // https://github.com/<repo>/releases/latest/download/latest.json
            // → https://github.com/<repo>/releases/download/v<version>/<file>
            let base = self
                .endpoint
                .trim_end_matches("/latest/download/latest.json");
            format!("{base}/download/v{version}/{filename}")
        } else {
            // 官方源：与 endpoint 同主机的 /updates/releases/<variant>/<version>/<file>
            let base = self
                .endpoint
                .trim_end_matches(&format!("/updates/{}/stable.json", self.variant));
            format!(
                "{base}/updates/releases/{}/{version}/{filename}",
                self.variant
            )
        };
        manifest.get("schemaVersion").and_then(|v| v.as_u64()) == Some(2)
            && manifest.get("variant").and_then(|v| v.as_str()) == Some(self.variant)
            && manifest.get("version").and_then(|v| v.as_str()) == Some(version)
            && url == expected_url
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FIXTURE: &str = r#"{
      "schemaVersion": 1,
      "official": {"authorizationOrigin": "https://yeschoy.com",
                   "endpoint": "https://updates.example/updates/official/stable.json",
                   "publicKey": "official-key"},
      "partner": {"authorizationOrigin": "https://ai.yeschoy.io",
                  "endpoint": "https://updates.example/updates/partner/stable.json",
                  "publicKey": "partner-key"}
    }"#;

    fn channel_for_origin(origin: &str) -> Result<Channel, &'static str> {
        super::channel_for_origin(FIXTURE, origin)
    }

    #[test]
    fn community_channel_needs_a_key_and_uses_github_release_urls() {
        assert!(community_channel("o/r", "").is_err());
        assert!(community_channel("bad repo", "k").is_err());
        let channel = community_channel("o/r", "k").unwrap();
        assert_eq!(
            channel.endpoint,
            "https://github.com/o/r/releases/latest/download/latest.json"
        );
        let suffix = if cfg!(target_os = "windows") {
            "windows-x86_64-installer.exe"
        } else {
            "macos-universal.app.tar.gz"
        };
        let url = format!(
            "https://github.com/o/r/releases/download/v1.2.3/yeschoy-1.2.3-community-{suffix}"
        );
        let manifest = json!({"schemaVersion": 2, "variant": "community", "version": "1.2.3"});
        assert!(channel.accepts(&manifest, &url, "1.2.3"));
        assert!(!channel.accepts(&manifest, &url.replace("o/r", "evil/r"), "1.2.3"));
    }

    #[test]
    fn variants_use_disjoint_fixed_feeds_and_keys() {
        let official = channel_for_origin("https://yeschoy.com").unwrap();
        let partner = channel_for_origin("https://ai.yeschoy.io").unwrap();
        assert_ne!(official.endpoint, partner.endpoint);
        assert_ne!(official.public_key, partner.public_key);
        assert!(channel_for_origin("https://evil.invalid").is_err());
    }

    #[test]
    fn wrong_variant_old_manifest_and_foreign_urls_are_rejected() {
        let channel = channel_for_origin("https://ai.yeschoy.io").unwrap();
        let version = "0.4.17";
        let suffix = if cfg!(target_os = "windows") {
            "windows-x86_64-installer.exe"
        } else {
            "macos-universal.app.tar.gz"
        };
        let url = format!("https://updates.example/updates/releases/partner/{version}/yeschoy-{version}-partner-{suffix}");
        let manifest = json!({"schemaVersion": 2, "variant": "partner", "version": version});
        assert!(channel.accepts(&manifest, &url, version));
        assert!(!channel.accepts(&json!({"version": version}), &url, version));
        assert!(!channel.accepts(
            &json!({"schemaVersion": 2, "variant": "official", "version": version}),
            &url,
            version
        ));
        assert!(!channel.accepts(&manifest, &url.replace("partner", "official"), version));
        assert!(!channel.accepts(&manifest, &format!("{url}?redirect=other"), version));
        assert!(!channel.accepts(&manifest, &url.replace("https:", "http:"), version));
    }
}
