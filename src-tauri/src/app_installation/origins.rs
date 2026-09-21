//! Public installer metadata is untrusted. Compiled vendor identity and native
//! signature checks remain authoritative; no account client is used here.
use super::{
    catalog::{self, Source},
    Result,
};
use reqwest::{Client, Url};
use serde::Deserialize;

pub(super) const MIRROR_HOST: &str = "ergou.qzz.io";
pub(super) const MIRROR_ORIGIN: &str = "43.134.77.210:443";
pub(super) const CATALOG_URL: &str = "https://ergou.qzz.io/apps/catalog.json";
const JSON_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub(super) struct Resolved {
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub mirror: bool,
}
impl Resolved {
    pub fn official(url: &str) -> Self {
        Self {
            url: url.into(),
            sha256: String::new(),
            size: 0,
            mirror: false,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Catalog {
    schema_version: u8,
    generated_at: String,
    artifacts: Vec<Artifact>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Artifact {
    source_id: String,
    app: String,
    platform: String,
    architecture: String,
    format: String,
    version: String,
    url: String,
    sha256: String,
    size: u64,
    origin_url: String,
    identity: String,
    publisher: String,
    verified_at: String,
    verification: String,
}

fn version(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    (2..=4).contains(&parts.len())
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 10 && p.bytes().all(|c| c.is_ascii_digit()))
}
fn timestamp(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && value.bytes().all(|c| c.is_ascii_graphic())
}

fn compiled(artifact: &Artifact, schema_version: u8) -> Option<Source> {
    let tool = match artifact.app.as_str() {
        "codex" => "codex_desktop",
        "claude" => "claude_desktop",
        _ => return None,
    };
    let arch = if artifact.architecture == "universal" {
        "arm64"
    } else {
        &artifact.architecture
    };
    let source = catalog::source(tool, &artifact.platform, arch)?;
    if catalog::source_id(source) != artifact.source_id
        || source.extension != artifact.format
        || (source.extension == "zip") != (artifact.architecture == "universal")
        || source.identity != artifact.identity
        || source.publisher != artifact.publisher
        || !matches!(
            (schema_version, artifact.verification.as_str()),
            (1, "native_verified") | (2, "native_verified") | (2, "client_native_required")
        )
        || !version(&artifact.version)
        || !timestamp(&artifact.verified_at)
        || !(1..=super::download::MAX_BYTES).contains(&artifact.size)
        || artifact.sha256.len() != 64
        || !artifact
            .sha256
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        || artifact.url
            != format!(
                "https://ergou.qzz.io/apps/{}/{}.{}",
                artifact.source_id, artifact.sha256, artifact.format
            )
        || !Url::parse(&artifact.origin_url).is_ok_and(|url| {
            catalog::allowed_url(source, &url)
                && url.path().ends_with(&format!(".{}", source.extension))
                && (source.extension != "zip"
                    || url
                        .path()
                        .starts_with(&format!("/releases/darwin/universal/{}/", artifact.version)))
        })
    {
        return None;
    }
    Some(source)
}

pub(super) fn parse_catalog(bytes: &[u8], source: Source) -> Result<Option<Resolved>> {
    if bytes.len() > JSON_LIMIT {
        return Err("invalid_source");
    }
    let catalog: Catalog = serde_json::from_slice(bytes).map_err(|_| "invalid_source")?;
    if !matches!(catalog.schema_version, 1 | 2)
        || !timestamp(&catalog.generated_at)
        || catalog.artifacts.len() > 6
    {
        return Err("invalid_source");
    }
    let mut seen = std::collections::HashSet::new();
    let mut selected = None;
    for item in catalog.artifacts {
        if compiled(&item, catalog.schema_version).is_none() || !seen.insert(item.source_id.clone())
        {
            return Err("invalid_source");
        }
        if item.source_id == catalog::source_id(source) {
            selected = Some(Resolved {
                url: item.url,
                sha256: item.sha256,
                size: item.size,
                mirror: true,
            });
        }
    }
    Ok(selected)
}

// Vendor metadata can add irrelevant fields. Select exactly one matching
// currentRelease; never take the first historical item or an arbitrary URL.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Feed {
    current_release: String,
    releases: Vec<Release>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Release {
    version: String,
    update_to: Update,
}
#[derive(Deserialize)]
struct Update {
    version: String,
    url: String,
}
pub(super) fn parse_claude_feed(bytes: &[u8], source: Source) -> Result<Resolved> {
    if source.tool != "claude_desktop" || source.extension != "zip" || bytes.len() > JSON_LIMIT {
        return Err("invalid_source");
    }
    let value: Feed = serde_json::from_slice(bytes).map_err(|_| "invalid_source")?;
    let current = &value.current_release;
    if !version(current) {
        return Err("invalid_source");
    }
    let matching: Vec<_> = value
        .releases
        .iter()
        .filter(|r| &r.version == current)
        .collect();
    if matching.len() != 1 {
        return Err("invalid_source");
    }
    let update = &matching[0].update_to;
    let url_text = update.url.as_str();
    let url = Url::parse(url_text).map_err(|_| "invalid_source")?;
    let prefix = format!("/releases/darwin/universal/{current}/");
    let tail = url.path().strip_prefix(&prefix).ok_or("invalid_source")?;
    if &update.version != current
        || !catalog::allowed_url(source, &url)
        || !tail.starts_with("Claude-")
        || tail.contains('/')
        || url.as_str() != url_text
    {
        return Err("invalid_source");
    }
    Ok(Resolved::official(url_text))
}

async fn json(client: &Client, url: &str) -> Result<Vec<u8>> {
    let mut response = client
        .get(url)
        .header("accept", "application/json")
        .header("accept-encoding", "identity")
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|_| "network_unavailable")?;
    if response.status() != reqwest::StatusCode::OK
        || response.headers().get("cf-mitigated").is_some()
        || response
            .content_length()
            .is_some_and(|n| n > JSON_LIMIT as u64)
    {
        return Err("source_unavailable");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "network_unavailable")? {
        if chunk.len() > JSON_LIMIT - bytes.len() {
            return Err("invalid_source");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
async fn mirror_at(client: &Client, source: Source, url: &str) -> Result<Option<Resolved>> {
    parse_catalog(&json(client, url).await?, source)
}
pub(super) async fn mirror_from_routes(
    public: &Client,
    direct: &Client,
    source: Source,
) -> Result<Option<Resolved>> {
    mirror_from_routes_at(public, direct, source, CATALOG_URL).await
}
async fn mirror_from_routes_at(
    public: &Client,
    direct: &Client,
    source: Source,
    url: &str,
) -> Result<Option<Resolved>> {
    match mirror_at(public, source, url).await {
        Ok(value) => Ok(value),
        Err(_) => mirror_at(direct, source, url).await,
    }
}
pub(super) async fn official(client: &Client, source: Source) -> Result<Resolved> {
    if source.extension == "zip" {
        parse_claude_feed(&json(client, source.url).await?, source)
    } else {
        Ok(Resolved::official(source.url))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn artifact() -> serde_json::Value {
        json!({"sourceId":"claude-macos-universal","app":"claude","platform":"macos","architecture":"universal","format":"zip","version":"1.46388.4","url":format!("https://ergou.qzz.io/apps/claude-macos-universal/{}.zip", "a".repeat(64)),"sha256":"a".repeat(64),"size":355648442,"originUrl":"https://downloads.claude.ai/releases/darwin/universal/1.46388.4/Claude-example.zip","identity":"com.anthropic.claudefordesktop","publisher":"Q6L2SF6YDW","verifiedAt":"2026-09-06T09:00:00Z","verification":"native_verified"})
    }

    #[tokio::test]
    async fn mirror_catalog_uses_direct_route_only_after_public_transport_failure() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).await.unwrap();
                request.push(byte[0]);
            }
            let body = serde_json::to_vec(&json!({
                "schemaVersion": 1,
                "generatedAt": "2026-09-16T08:31:00Z",
                "artifacts": [artifact()]
            }))
            .unwrap();
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            socket.write_all(&body).await.unwrap();
            String::from_utf8(request).unwrap()
        });
        // Both fixtures live on 127.0.0.1. A developer machine with a system
        // proxy configured would otherwise route "catalog.test" through it,
        // and the `resolve` overrides never apply to a proxied request.
        let public = Client::builder()
            .no_proxy()
            .resolve("catalog.test", "127.0.0.1:1".parse().unwrap())
            .build()
            .unwrap();
        let direct = Client::builder()
            .no_proxy()
            .resolve("catalog.test", address)
            .build()
            .unwrap();
        let source = catalog::source("claude_desktop", "macos", "x64").unwrap();
        let selected = mirror_from_routes_at(
            &public,
            &direct,
            source,
            "http://catalog.test/apps/catalog.json",
        )
        .await
        .unwrap()
        .unwrap();
        assert!(selected.mirror);
        let request = server.await.unwrap();
        assert!(request.starts_with("GET /apps/catalog.json"));
        assert!(!request.to_ascii_lowercase().contains("authorization:"));
        assert!(!request.to_ascii_lowercase().contains("cookie:"));
    }
    #[test]
    fn installer_manifest_is_closed_and_exact_vendor_bound() {
        let source = catalog::source("claude_desktop", "macos", "x64").unwrap();
        let valid = json!({"schemaVersion":1,"generatedAt":"2026-09-06T09:00:00Z","artifacts":[artifact()]});
        let parse = |v: &serde_json::Value| parse_catalog(&serde_json::to_vec(v).unwrap(), source);
        let result = parse(&valid).unwrap().unwrap();
        assert!(result.mirror);
        assert_eq!(result.size, 355648442);
        let mut requires_client_native = valid.clone();
        requires_client_native["artifacts"][0]["verification"] = json!("client_native_required");
        assert!(parse(&requires_client_native).is_err());
        requires_client_native["schemaVersion"] = json!(2);
        assert!(parse(&requires_client_native).unwrap().is_some());
        let mut unsupported_schema = requires_client_native.clone();
        unsupported_schema["schemaVersion"] = json!(3);
        assert!(parse(&unsupported_schema).is_err());
        for (key, bad) in [
            ("app", json!("other")),
            ("platform", json!("windows")),
            ("architecture", json!("x64")),
            ("format", json!("dmg")),
            ("identity", json!("other")),
            ("publisher", json!("other")),
            ("verification", json!("downloaded")),
            ("sha256", json!("A".repeat(64))),
            ("size", json!(0)),
            ("originUrl", json!("https://evil.example/a.zip")),
            ("originUrl", json!("https://downloads.claude.ai/releases/darwin/universal/1.0.0/Claude-example.zip")),
            (
                "url",
                json!(format!(
                    "https://ergou.qzz.io.evil.example/apps/claude-macos-universal/{}.zip",
                    "a".repeat(64)
                )),
            ),
            ("unexpected", json!(true)),
        ] {
            let mut value = valid.clone();
            value["artifacts"][0][key] = bad;
            assert!(parse(&value).is_err(), "{key}");
        }
        let mut duplicate = valid.clone();
        duplicate["artifacts"]
            .as_array_mut()
            .unwrap()
            .push(artifact());
        assert!(parse(&duplicate).is_err());
        let mut empty = valid;
        empty["artifacts"] = json!([]);
        assert!(parse(&empty).unwrap().is_none());
    }
    #[test]
    fn installer_manifest_origin_must_be_the_package_not_the_resolver() {
        let source = catalog::source("claude_desktop", "windows", "x64").unwrap();
        let mut entry = artifact();
        entry["sourceId"] = json!("claude-windows-x64");
        entry["platform"] = json!("windows");
        entry["architecture"] = json!("x64");
        entry["format"] = json!("msix");
        entry["identity"] = json!(source.identity);
        entry["publisher"] = json!(source.publisher);
        entry["url"] = json!(format!(
            "https://ergou.qzz.io/apps/claude-windows-x64/{}.msix",
            "a".repeat(64)
        ));
        entry["originUrl"] =
            json!("https://downloads.claude.ai/releases/win32/x64/1.46388.4/Claude.msix");
        let mut value =
            json!({"schemaVersion":1,"generatedAt":"2026-09-06T09:00:00Z","artifacts":[entry]});
        assert!(parse_catalog(&serde_json::to_vec(&value).unwrap(), source)
            .unwrap()
            .is_some());
        value["artifacts"][0]["originUrl"] = json!(source.url);
        assert!(parse_catalog(&serde_json::to_vec(&value).unwrap(), source).is_err());
    }
    #[test]
    fn installer_claude_feed_uses_only_exact_current_official_release() {
        let source = catalog::source("claude_desktop", "macos", "x64").unwrap();
        let valid = json!({"currentRelease":"1.46388.4","releases":[{"version":"1.1.1","updateTo":{"version":"1.1.1","url":"https://evil.example/a.zip"}},{"version":"1.46388.4","updateTo":{"version":"1.46388.4","url":"https://downloads.claude.ai/releases/darwin/universal/1.46388.4/Claude-example.zip"}}]});
        let parse =
            |v: &serde_json::Value| parse_claude_feed(&serde_json::to_vec(v).unwrap(), source);
        assert!(parse(&valid).unwrap().url.contains("1.46388.4/Claude-"));
        for bad in ["https://evil.example/a.zip", "https://downloads.claude.ai/releases/darwin/universal/1.1.1/Claude-example.zip", "https://downloads.claude.ai/releases/darwin/universal/1.46388.4/Claude-example.zip?token=x", "https://downloads.claude.ai/releases/darwin/universal/1.46388.4/Claude-example.zip#x", "https://user:pass@downloads.claude.ai/releases/darwin/universal/1.46388.4/Claude-example.zip"] {
            let mut v = valid.clone(); v["releases"][1]["updateTo"]["url"] = json!(bad); assert!(parse(&v).is_err());
        }
        let mut v = valid.clone();
        v["releases"][1]["updateTo"]["version"] = json!("1.1.1");
        assert!(parse(&v).is_err());
        let mut v = valid;
        let repeated = v["releases"][1].clone();
        v["releases"].as_array_mut().unwrap().push(repeated);
        assert!(parse(&v).is_err());
        assert!(parse_claude_feed(&vec![b' '; JSON_LIMIT + 1], source).is_err());
        assert!(parse_claude_feed(
            br#"{"currentRelease":"1.0","currentRelease":"2.0","releases":[]}"#,
            source
        )
        .is_err());
    }
}
