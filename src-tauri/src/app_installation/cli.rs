//! 命令行工具（Claude Code、OpenCode、Pi）的安装。
//!
//! 只做三件事：按官方的最新版本找到这台电脑该用的那个文件，下载后核对官方给的
//! SHA-256，放进客户端自己的目录。不跑安装脚本、不要 Node、不要管理员权限，
//! 卸载就是删掉那个目录。
//!
//! 校验值和文件来自同一个官方源：它能发现文件损坏、截断或中途被换，
//! 但不等于核实了发行方的签名。
use super::{
    cache,
    catalog::Source,
    origins::{self, Resolved},
    Result,
};
use reqwest::{Client, Url};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

const CLAUDE: &str = "https://downloads.claude.ai/claude-code-releases";
const PI: &str = "earendil-works/pi";
// OpenCode 官方把同一批程序也发在 npm 上，那里的清单带校验值而且不限次数；
// GitHub 的发行接口每个网络出口每小时只让查 60 次，共用出口的人很容易查不到。
const NPM: &str = "https://registry.npmjs.org";
const TEXT_LIMIT: usize = 64 * 1024;
const MAX_ENTRIES: usize = 20_000;
const MAX_UNPACKED: u64 = 2 * 1024 * 1024 * 1024;

fn tools_root() -> Option<PathBuf> {
    Some(crate::legacy::data_root(&crate::tool_adapters::user_home()?).join("tools"))
}

/// 客户端自己装的命令行工具所在的目录，交给应用发现去找。
pub(crate) fn owned_directories() -> Vec<PathBuf> {
    tools_root()
        .map(|root| {
            ["claude_code", "opencode", "pi"]
                .iter()
                .map(|tool| root.join(tool))
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn owns(tool: &str) -> bool {
    matches!(tool, "claude_code" | "opencode" | "pi")
}

fn executable(source: Source) -> String {
    let name = crate::tool_adapters::executable_for(source.tool).unwrap_or(source.tool);
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn version_like(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 40
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_'))
        && !value.contains("..")
}

/// 下载和取校验值时允许访问的地址。重定向的每一跳都要过这一关。
pub(super) fn allowed_url(source: Source, url: &Url) -> bool {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default() != Some(443)
        || url.path().contains('%')
        || url.path().contains('\\')
        || url.path().contains("..")
    {
        return false;
    }
    let repos: &[&str] = match source.tool {
        "pi" => &[PI],
        _ => &[],
    };
    match url.host_str() {
        Some("downloads.claude.ai") => {
            source.tool == "claude_code"
                && url.query().is_none()
                && url.path().starts_with("/claude-code-releases/")
        }
        Some("registry.npmjs.org") => {
            source.tool == "opencode"
                && url.query().is_none()
                && url.path().starts_with("/opencode-")
                && url.path().ends_with(".tgz")
                && url.path().contains("/-/opencode-")
        }
        Some("github.com") => {
            url.query().is_none()
                && repos.iter().any(|repo| {
                    url.path()
                        .starts_with(&format!("/{repo}/releases/download/"))
                })
        }
        // GitHub 发行文件的存放处；地址带一次性的签名参数。
        Some("release-assets.githubusercontent.com") => {
            !repos.is_empty() && url.path().starts_with("/github-production-release-asset/")
        }
        _ => false,
    }
}

/// 取一小段文字（版本号、校验清单），手动跟随最多 5 次重定向，每一跳都过白名单。
async fn text(client: &Client, source: Source, url: &str) -> Result<String> {
    let mut url = Url::parse(url).map_err(|_| "invalid_source")?;
    for _ in 0..6 {
        if !allowed_url(source, &url) {
            return Err("invalid_source");
        }
        let mut response = client
            .get(url.clone())
            .header("accept-encoding", "identity")
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|_| "network_unavailable")?;
        if response.status().is_redirection() {
            let next = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or("invalid_source")?;
            url = url.join(next).map_err(|_| "invalid_source")?;
            continue;
        }
        if response.status() != reqwest::StatusCode::OK {
            return Err("source_unavailable");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "network_unavailable")? {
            if chunk.len() > TEXT_LIMIT - bytes.len() {
                return Err("invalid_source");
            }
            bytes.extend_from_slice(&chunk);
        }
        return String::from_utf8(bytes).map_err(|_| "invalid_source");
    }
    Err("invalid_source")
}

/// GitHub 把 `releases/latest` 重定向到带版本号的页面；只读那个版本号。
async fn latest_tag(client: &Client, repo: &str) -> Result<String> {
    let response = client
        .get(format!("https://github.com/{repo}/releases/latest"))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|_| "network_unavailable")?;
    if !response.status().is_redirection() {
        return Err("source_unavailable");
    }
    let location = response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .ok_or("invalid_source")?;
    tag_from_location(repo, location)
}

fn tag_from_location(repo: &str, location: &str) -> Result<String> {
    location
        .strip_prefix(&format!("https://github.com/{repo}/releases/tag/"))
        .filter(|tag| version_like(tag))
        .map(str::to_owned)
        .ok_or("invalid_source")
}

fn claude_platform(os: &str, arch: &str) -> Option<String> {
    let os = match os {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "win32",
        _ => return None,
    };
    Some(format!("{os}-{arch}"))
}

pub(super) fn parse_claude_manifest(
    bytes: &[u8],
    version: &str,
    os: &str,
    arch: &str,
) -> Result<Resolved> {
    let platform = claude_platform(os, arch).ok_or("unsupported_platform")?;
    let manifest: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "invalid_source")?;
    let entry = &manifest["platforms"][&platform];
    let sha256 = entry["checksum"].as_str().unwrap_or("");
    let size = entry["size"].as_u64().unwrap_or(0);
    let binary = if os == "windows" {
        "claude.exe"
    } else {
        "claude"
    };
    if manifest["version"].as_str() != Some(version)
        || !hex64(sha256)
        || !(1..=super::download::MAX_BYTES).contains(&size)
        || entry["binary"].as_str() != Some(binary)
    {
        return Err("invalid_source");
    }
    Ok(Resolved {
        url: format!("{CLAUDE}/{version}/{platform}/{binary}"),
        sha256: sha256.to_owned(),
        size,
        mirror: false,
    })
}

fn asset_name(source: Source, os: &str) -> String {
    let os = match (source.tool, os) {
        (_, "macos") => "darwin",
        (_, "windows") => "windows",
        _ => "linux",
    };
    let extension = if source.extension == "zip" {
        "zip"
    } else {
        "tar.gz"
    };
    format!("{}-{os}-{}.{extension}", source.tool, source.architecture)
}

/// 这台电脑的处理器有没有 AVX2。不是 x64 的不用问。
fn has_avx2() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("avx2")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        true
    }
}

/// OpenCode 在 npm 上的平台包。x64 的标准版要用到 AVX2，老一些的处理器没有，
/// 装上也一运行就退出；官方给这些电脑另发了一份 `-baseline`。
pub(super) fn opencode_package(source: Source, os: &str, avx2: bool) -> String {
    let name = asset_name(source, os);
    let name = name.trim_end_matches(".tar.gz");
    if source.architecture == "x64" && !avx2 {
        format!("{name}-baseline")
    } else {
        name.to_owned()
    }
}

/// `SHA256SUMS`：每行「校验值␠␠文件名」。
pub(super) fn parse_sums(text: &str, name: &str) -> Result<String> {
    text.lines()
        .filter_map(|line| line.split_once(char::is_whitespace))
        .find(|(_, file)| file.trim().trim_start_matches('*') == name)
        .map(|(sum, _)| sum.to_owned())
        .filter(|sum| hex64(sum))
        .ok_or("invalid_source")
}

/// npm 清单里的一项：`dist.integrity` 是 `sha512-<base64>`。
pub(super) fn parse_npm_package(bytes: &[u8], name: &str) -> Result<Found> {
    let package: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| "invalid_source")?;
    let version = package["version"].as_str().unwrap_or("");
    let sha512 = package["dist"]["integrity"]
        .as_str()
        .and_then(|integrity| integrity.strip_prefix("sha512-"))
        .unwrap_or("");
    let url = format!("{NPM}/{name}/-/{name}-{version}.tgz");
    // 64 字节的 base64 正好 88 个字符。
    if package["name"].as_str() != Some(name)
        || !version_like(version)
        || sha512.len() != 88
        || package["dist"]["tarball"].as_str() != Some(url.as_str())
    {
        return Err("invalid_source");
    }
    Ok(Found {
        target: Resolved {
            url,
            sha256: String::new(),
            size: 0,
            mirror: false,
        },
        sha512: sha512.to_owned(),
    })
}

/// 官方给的文件和校验值。`target.sha256` 与 `sha512` 至少有一个，否则不装。
#[derive(Debug)]
pub(super) struct Found {
    pub(super) target: Resolved,
    pub(super) sha512: String,
}
impl Found {
    fn sha256(target: Resolved) -> Self {
        Self {
            target,
            sha512: String::new(),
        }
    }
    /// 下载下来的文件是不是官方说的那一个。
    pub(super) fn matches(&self, file: &Path, sha256: &str) -> Result<bool> {
        if !self.target.sha256.is_empty() {
            return Ok(sha256 == self.target.sha256);
        }
        if self.sha512.is_empty() {
            return Ok(false);
        }
        use base64::Engine;
        use sha2::Digest;
        let mut reader = fs::File::open(file).map_err(cache::io_error)?;
        let mut hash = sha2::Sha512::new();
        let mut bytes = [0u8; 64 * 1024];
        loop {
            let size = reader.read(&mut bytes).map_err(cache::io_error)?;
            if size == 0 {
                break;
            }
            hash.update(&bytes[..size]);
        }
        Ok(base64::engine::general_purpose::STANDARD.encode(hash.finalize()) == self.sha512)
    }
}

/// 找到官方最新版本里这台电脑该用的文件，连同官方给的校验值。没有校验值就不装。
pub(super) async fn resolve(client: &Client, source: Source, os: &str) -> Result<Found> {
    match source.tool {
        "claude_code" => {
            let version = text(client, source, &format!("{CLAUDE}/latest")).await?;
            let version = version.trim();
            if !version_like(version) {
                return Err("invalid_source");
            }
            let manifest =
                text(client, source, &format!("{CLAUDE}/{version}/manifest.json")).await?;
            parse_claude_manifest(manifest.as_bytes(), version, os, source.architecture)
                .map(Found::sha256)
        }
        "pi" => {
            let tag = latest_tag(client, PI).await?;
            let base = format!("https://github.com/{PI}/releases/download/{tag}");
            let name = asset_name(source, os);
            let sums = text(client, source, &format!("{base}/SHA256SUMS")).await?;
            Ok(Found::sha256(Resolved {
                url: format!("{base}/{name}"),
                sha256: parse_sums(&sums, &name)?,
                size: 0,
                mirror: false,
            }))
        }
        "opencode" => {
            let name = opencode_package(source, os, has_avx2());
            let package = origins::json(client, &format!("{NPM}/{name}/latest")).await?;
            parse_npm_package(&package, &name)
        }
        _ => Err("unsupported_platform"),
    }
}

/// 压缩包里的路径：只收普通的相对路径，`..`、绝对路径、盘符一律拒绝。
fn member(raw: &Path, strip: &[&str]) -> Result<Option<PathBuf>> {
    let mut parts = Vec::new();
    for part in raw.components() {
        match part {
            Component::Normal(name) => parts.push(name),
            Component::CurDir => (),
            _ => return Err("invalid_download"),
        }
    }
    // 包外面套着的那几层目录去掉；不在那几层里面的（比如 npm 包的 package.json）不要。
    if parts.len() <= strip.len() || !parts.iter().zip(strip).all(|(part, top)| part == top) {
        return Ok(None);
    }
    Ok(Some(parts[strip.len()..].iter().collect()))
}

struct Budget {
    entries: usize,
    bytes: u64,
}
impl Budget {
    fn entry(&mut self, size: u64) -> Result<()> {
        self.entries += 1;
        self.bytes = self.bytes.saturating_add(size);
        if self.entries > MAX_ENTRIES || self.bytes > MAX_UNPACKED {
            return Err("invalid_download");
        }
        Ok(())
    }
}

fn write_member(target: &Path, mut reader: impl Read, size: u64, runnable: bool) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(cache::io_error)?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(cache::io_error)?;
    // 条目自己报的大小就是上限：多出来的不写。
    let copied =
        std::io::copy(&mut (&mut reader).take(size), &mut file).map_err(cache::io_error)?;
    if copied != size {
        return Err("invalid_download");
    }
    file.flush().map_err(cache::io_error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if runnable { 0o755 } else { 0o644 };
        fs::set_permissions(target, fs::Permissions::from_mode(mode)).map_err(cache::io_error)?;
    }
    #[cfg(not(unix))]
    let _ = runnable;
    Ok(())
}

fn unpack_zip(file: &Path, into: &Path) -> Result<()> {
    let reader = fs::File::open(file).map_err(cache::io_error)?;
    let mut archive = zip::ZipArchive::new(reader).map_err(|_| "invalid_download")?;
    let mut budget = Budget {
        entries: 0,
        bytes: 0,
    };
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|_| "invalid_download")?;
        budget.entry(entry.size())?;
        let mode = entry.unix_mode().unwrap_or(0);
        if mode & 0o170000 == 0o120000 {
            return Err("invalid_download");
        }
        let raw = entry.enclosed_name().ok_or("invalid_download")?;
        let Some(path) = member(&raw, &[])? else {
            continue;
        };
        if entry.is_dir() {
            fs::create_dir_all(into.join(path)).map_err(cache::io_error)?;
            continue;
        }
        let size = entry.size();
        write_member(&into.join(path), entry, size, mode & 0o111 != 0)?;
    }
    Ok(())
}

fn unpack_tgz(file: &Path, into: &Path, strip: &[&str]) -> Result<()> {
    let reader = fs::File::open(file).map_err(cache::io_error)?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(reader));
    let mut budget = Budget {
        entries: 0,
        bytes: 0,
    };
    for entry in archive.entries().map_err(|_| "invalid_download")? {
        let entry = entry.map_err(|_| "invalid_download")?;
        let size = entry.header().size().map_err(|_| "invalid_download")?;
        budget.entry(size)?;
        let kind = entry.header().entry_type();
        let raw = entry.path().map_err(|_| "invalid_download")?.into_owned();
        let Some(path) = member(&raw, strip)? else {
            continue;
        };
        if kind.is_dir() {
            fs::create_dir_all(into.join(path)).map_err(cache::io_error)?;
        } else if kind.is_file() {
            let runnable = entry.header().mode().unwrap_or(0) & 0o111 != 0;
            write_member(&into.join(path), entry, size, runnable)?;
        } else {
            // 链接、设备之类的条目，官方包里没有，也不该有。
            return Err("invalid_download");
        }
    }
    Ok(())
}

/// 给新装的让出位置。空目录直接去掉；里面有东西就整个挪到旁边留着，不删。
/// 新的没摆好之前不会走到这一步，所以装不成时原来的文件一个都不会少。
fn make_room(root: &Path, tool: &str, destination: &Path) -> Result<()> {
    match fs::remove_dir(destination) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => fs::rename(
            destination,
            root.join(format!(".replaced-{tool}-{}", super::new_id()?)),
        )
        .map_err(cache::io_error),
    }
}

/// 把核对过的文件放进 `root/<tool>`：先在旁边的临时目录里摆好，最后一步改名。
/// 返回主程序的位置。
pub(super) fn install_at(root: &Path, source: Source, file: &Path) -> Result<PathBuf> {
    let name = executable(source);
    let destination = root.join(source.tool);
    match fs::symlink_metadata(&destination) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {
            if destination.join(&name).is_file() {
                return Err("already_installed");
            }
            // 目录在、主程序不在：里面的东西不是这次装的，先不碰，
            // 等新的摆好了再给它让位（见下面的 `make_room`）。
        }
        Ok(_) => return Err("installation_location_conflict"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(cache::io_error(error)),
    }
    let staging = root.join(format!(".staging-{}", super::new_id()?));
    fs::create_dir(&staging).map_err(cache::io_error)?;
    let placed = (|| {
        match source.extension {
            "bin" => {
                let reader = fs::File::open(file).map_err(cache::io_error)?;
                let size = reader.metadata().map_err(cache::io_error)?.len();
                write_member(&staging.join(&name), reader, size, true)?;
            }
            "zip" => unpack_zip(file, &staging)?,
            // Pi 的包外面套了一层 `pi/`；OpenCode 的 npm 包把主程序放在 `package/bin/`。
            _ if source.tool == "pi" => unpack_tgz(file, &staging, &["pi"])?,
            _ => unpack_tgz(file, &staging, &["package", "bin"])?,
        }
        let program = staging.join(&name);
        if !fs::symlink_metadata(&program).is_ok_and(|meta| meta.is_file() && meta.len() > 0) {
            return Err("invalid_download");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&program, fs::Permissions::from_mode(0o755))
                .map_err(cache::io_error)?;
        }
        make_room(root, source.tool, &destination)?;
        fs::rename(&staging, &destination).map_err(cache::io_error)
    })();
    if placed.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    placed.map(|()| destination.join(name))
}

pub(super) fn install(source: Source, file: &Path) -> Result<PathBuf> {
    let root = tools_root().ok_or("home_unavailable")?;
    let home = root.parent().ok_or("home_unavailable")?;
    cache::directory(home)?;
    cache::directory(&root)?;
    install_at(&root, source, file)
}
