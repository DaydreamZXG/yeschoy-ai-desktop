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

pub(crate) fn compiled_channel() -> Result<Channel, &'static str> {
    channel_for_origin(env!("YESCHOY_AUTHORIZATION_PAGE_ORIGIN"))
}

fn channel_for_origin(origin: &str) -> Result<Channel, &'static str> {
    let registry: Registry = serde_json::from_str(include_str!("../update-channels.json"))
        .map_err(|_| "channel_invalid")?;
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
        || channel.endpoint != format!("https://ergou.qzz.io/updates/{variant}/stable.json")
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
        manifest.get("schemaVersion").and_then(|v| v.as_u64()) == Some(2)
            && manifest.get("variant").and_then(|v| v.as_str()) == Some(self.variant)
            && manifest.get("version").and_then(|v| v.as_str()) == Some(version)
            && url
                == format!(
                    "https://ergou.qzz.io/updates/releases/{}/{version}/{filename}",
                    self.variant
                )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        let url = format!("https://ergou.qzz.io/updates/releases/partner/{version}/yeschoy-{version}-partner-{suffix}");
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
