use super::{
    cache,
    catalog::{allowed_url, Source},
    origins::{self, Resolved},
    Result, Worker,
};
use reqwest::{header::HeaderMap, StatusCode, Url};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

pub(super) const MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;
#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    etag: String,
    total: u64,
    sha256: String,
    #[serde(default)]
    artifact_url: String,
    #[serde(default)]
    final_url: String,
    #[serde(default)]
    expected_sha256: String,
    #[serde(default)]
    expected_size: u64,
}

pub(super) fn strong_etag(value: &str) -> bool {
    value.len() >= 2
        && value.len() <= 202
        && value.starts_with('"')
        && value.ends_with('"')
        && value[1..value.len() - 1]
            .bytes()
            .all(|b| b >= 32 && b != b'"' && b != 127)
}

pub(super) fn response_size(
    headers: &HeaderMap,
    status: StatusCode,
    offset: u64,
    expected: u64,
) -> Result<u64> {
    let text = |name| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
    };
    let kind = text("content-type").split(';').next().unwrap_or("").trim();
    if ![
        "application/octet-stream",
        "application/x-apple-diskimage",
        "application/vnd.ms-appx",
        "application/msix",
        "application/zip",
    ]
    .contains(&kind)
        || !matches!(text("content-encoding"), "" | "identity")
    {
        return Err("invalid_download");
    }
    let length: u64 = text("content-length")
        .parse()
        .map_err(|_| "invalid_download")?;
    if length == 0 || length > MAX_BYTES {
        return Err("invalid_download");
    }
    if status == StatusCode::OK && offset == 0 {
        return Ok(length);
    }
    if status != StatusCode::PARTIAL_CONTENT
        || offset == 0
        || expected > MAX_BYTES
        || offset >= expected
        || length != expected - offset
        || text("content-range") != format!("bytes {offset}-{}/{expected}", expected - 1)
    {
        return Err("download_changed");
    }
    Ok(expected)
}

pub(super) async fn fetch(
    worker: &Worker,
    source: Source,
    folder: &Path,
) -> Result<(PathBuf, String)> {
    let public = build_client(None)?;
    // Direct client is no_proxy plus compiled resolve of MIRROR_HOST only.
    let direct = build_client(Some(origins::MIRROR_ORIGIN))?;
    let mirror = origins::mirror_from_routes(&public, &direct, source)
        .await
        .ok()
        .flatten();
    fetch_from_sources(
        worker,
        source,
        folder,
        &public,
        Some(&direct),
        mirror,
        origins::official(&public, source),
        allowed_url,
    )
    .await
}

fn build_client(direct_origin: Option<&str>) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(30))
        .user_agent(concat!("Yecai-Desktop/", env!("CARGO_PKG_VERSION")));
    if let Some(address) = direct_origin {
        builder = builder.no_proxy().resolve(
            origins::MIRROR_HOST,
            address.parse().map_err(|_| "invalid_source")?,
        );
    }
    builder.build().map_err(|_| "network_unavailable")
}

// Resolve the official feed lazily: a usable mirror never needs to contact the
// blocked vendor host. Mirror failure never changes the selected application.
// Every argument here is an independent decision the caller has already made:
// which worker reports progress, which of two clients to use, whether a mirror
// was resolved, how to reach the official source, and what counts as an
// acceptable URL. Bundling them into one struct would hide that they are
// independent without removing a single one.
#[expect(
    clippy::too_many_arguments,
    reason = "independent caller decisions; a params struct would obscure, not simplify"
)]
pub(super) async fn fetch_from_sources(
    worker: &Worker,
    source: Source,
    folder: &Path,
    client: &reqwest::Client,
    direct_mirror_client: Option<&reqwest::Client>,
    mirror: Option<Resolved>,
    official: impl std::future::Future<Output = Result<Resolved>>,
    url_policy: fn(Source, &Url) -> bool,
) -> Result<(PathBuf, String)> {
    if let Some(target) = mirror {
        worker.update(|j| j.progress.source = "mirror");
        let mut routes = vec![client];
        if let Some(direct) = direct_mirror_client {
            routes.push(direct);
        }
        for route in routes {
            match fetch_resolved(worker, source, folder, route, url_policy, &target).await {
                Ok(file) => return Ok(file),
                Err(reason) if local_failure(reason) => return Err(reason),
                Err(_) => worker.bytes(0, 0),
            }
        }
    }
    worker.check_cancelled()?;
    worker.update(|j| j.progress.source = "official");
    let target = official.await?;
    fetch_resolved(worker, source, folder, client, url_policy, &target).await
}

fn local_failure(reason: &str) -> bool {
    matches!(
        reason,
        "cancelled" | "disk_full" | "permission_denied" | "unsafe_cache" | "cache_unavailable"
    )
}

// The public/native command never accepts this policy or client. Tests exercise
// the identical receive/retry loop against an isolated local HTTP fixture.
#[cfg(test)]
pub(super) async fn fetch_with_client(
    worker: &Worker,
    source: Source,
    folder: &Path,
    client: &reqwest::Client,
    url_policy: fn(Source, &Url) -> bool,
) -> Result<(PathBuf, String)> {
    fetch_resolved(
        worker,
        source,
        folder,
        client,
        url_policy,
        &Resolved::official(source.url),
    )
    .await
}

pub(super) async fn fetch_resolved(
    worker: &Worker,
    source: Source,
    folder: &Path,
    client: &reqwest::Client,
    url_policy: fn(Source, &Url) -> bool,
    target: &Resolved,
) -> Result<(PathBuf, String)> {
    cache::directory(folder)?;
    let file = folder.join(format!("installer.{}", source.extension));
    let meta_file = folder.join("download.json");
    let mut last = "network_unavailable";
    for _ in 0..3 {
        worker.check_cancelled()?;
        let mut meta: Metadata = if cache::regular(&meta_file)?.is_some_and(|len| len <= 4096) {
            cache::read_small(&meta_file)
                .ok()
                .and_then(|v| serde_json::from_slice(&v).ok())
                .unwrap_or_default()
        } else {
            Metadata::default()
        };
        // An ETag is scoped to one resource, not globally unique. Never append
        // or reuse a different mirror/version/source even if validators match.
        if meta.artifact_url != target.url
            || meta.expected_sha256 != target.sha256
            || meta.expected_size != target.size
        {
            meta = Metadata::default();
        }
        let size = cache::regular(&file)?.unwrap_or(0);
        let offset = if strong_etag(&meta.etag)
            && !meta.final_url.is_empty()
            && size > 0
            && size < meta.total
            && meta.total <= MAX_BYTES
        {
            size
        } else {
            0
        };
        let attempt = async {
            let mut url = Url::parse(&target.url).map_err(|_| "invalid_source")?;
            let mut response = None;
            for _ in 0..6 {
                if if target.mirror {
                    url.as_str() != target.url
                } else {
                    !url_policy(source, &url)
                        || (source.extension == "zip" && url.as_str() != target.url)
                } {
                    return Err("invalid_source");
                }
                let mut request = client
                    .get(url.clone())
                    .header("accept-encoding", "identity");
                let request_offset = if url.as_str() == meta.final_url {
                    offset
                } else {
                    0
                };
                if request_offset > 0 {
                    request = request
                        .header("range", format!("bytes={request_offset}-"))
                        .header("if-range", &meta.etag);
                }
                let fetched = request.send().await.map_err(|_| "network_unavailable")?;
                if fetched.status().is_redirection() {
                    let next = fetched
                        .headers()
                        .get("location")
                        .and_then(|v| v.to_str().ok())
                        .ok_or("invalid_source")?;
                    url = url.join(next).map_err(|_| "invalid_source")?;
                    continue;
                }
                response = Some((fetched, url.to_string(), request_offset));
                break;
            }
            let (mut response, final_url, request_offset) = response.ok_or("invalid_source")?;
            if response.headers().get("cf-mitigated").is_some() {
                return Err("source_challenge");
            }
            if !matches!(
                response.status(),
                StatusCode::OK | StatusCode::PARTIAL_CONTENT
            ) {
                return Err("source_unavailable");
            }
            let received_offset = if response.status() == StatusCode::OK {
                0
            } else {
                request_offset
            };
            let total = response_size(
                response.headers(),
                response.status(),
                received_offset,
                meta.total,
            )?;
            if target.size != 0 && total != target.size {
                return Err("download_changed");
            }
            let etag = response
                .headers()
                .get("etag")
                .and_then(|v| v.to_str().ok())
                .filter(|v| strong_etag(v))
                .unwrap_or("")
                .to_string();
            if received_offset > 0 && etag != meta.etag {
                return Err("download_changed");
            }
            if size == total
                && !etag.is_empty()
                && meta.etag == etag
                && meta.final_url == final_url
                && meta.total == total
                && meta.sha256.len() == 64
                && (target.sha256.is_empty() || meta.sha256 == target.sha256)
                && cache::digest(&file)? == meta.sha256
            {
                worker.bytes(total, total);
                return Ok((file.clone(), meta.sha256));
            }
            let mut metadata = Metadata {
                etag,
                total,
                sha256: String::new(),
                artifact_url: target.url.clone(),
                final_url,
                expected_sha256: target.sha256.clone(),
                expected_size: target.size,
            };
            // For a fresh response retire old bytes before publishing its new
            // validator. A crash must never label old bytes with a new ETag.
            let mut output = cache::open(&file, received_offset > 0)?;
            output.sync_all().map_err(cache::io_error)?;
            cache::write(
                &meta_file,
                &serde_json::to_vec(&metadata).map_err(|_| "cache_unavailable")?,
            )?;
            let mut received = received_offset;
            worker.bytes(received, total);
            while let Some(chunk) = response.chunk().await.map_err(|_| "network_unavailable")? {
                worker.check_cancelled()?;
                if chunk.len() as u64 > total - received {
                    return Err("invalid_download");
                }
                output.write_all(&chunk).map_err(cache::io_error)?;
                received += chunk.len() as u64;
                worker.bytes(received, total);
            }
            output.sync_all().map_err(cache::io_error)?;
            drop(output);
            if received != total {
                return Err("download_incomplete");
            }
            metadata.sha256 = cache::digest(&file)?;
            if !target.sha256.is_empty() && metadata.sha256 != target.sha256 {
                cache::write(&meta_file, b"{}")?;
                return Err("invalid_download");
            }
            cache::write(
                &meta_file,
                &serde_json::to_vec(&metadata).map_err(|_| "cache_unavailable")?,
            )?;
            Ok((file.clone(), metadata.sha256))
        }
        .await;
        match attempt {
            Ok(value) => return Ok(value),
            Err(reason) => {
                last = reason;
                if !matches!(
                    reason,
                    "network_unavailable" | "download_incomplete" | "download_changed"
                ) {
                    break;
                }
                if reason == "download_changed" {
                    cache::write(&meta_file, b"{}")?;
                }
            }
        }
    }
    Err(last)
}
