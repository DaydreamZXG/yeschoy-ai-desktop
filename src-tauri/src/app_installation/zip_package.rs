//! The Claude ZIP is data, not an extraction program. Validate the entire
//! namespace before creating anything, and never let a ZIP library choose paths.
#![cfg(any(target_os = "macos", test))]

use super::{cache, download::MAX_BYTES, Result};
use caseless::Caseless;
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use unicode_normalization::UnicodeNormalization;
use zip::{CompressionMethod, ZipArchive};

// Classic ZIP's entry-count limit and Darwin's PATH_MAX. ZIP64, multi-disk and
// self-extracting archives are not needed by this compiled vendor source.
const MAX_ENTRIES: usize = u16::MAX as usize - 1;
const MAX_PATH_BYTES: usize = 1024;
const MAX_SYMLINKS: usize = 32; // Darwin sys/param.h, MAXSYMLINKS.
                                // Bound the library's retained header/extra/comment allocation separately from
                                // file expansion, using the same entry and pathname budgets.
const MAX_DIRECTORY_BYTES: u64 = (MAX_ENTRIES * (MAX_PATH_BYTES + 46)) as u64;
const APP: &str = "Claude.app";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Directory,
    File,
    Link,
}

#[derive(Debug)]
struct Entry {
    name: String,
    kind: Kind,
    mode: u32,
    size: u64,
    compressed: u64,
    header: u64,
    link: Option<String>,
}

#[derive(Debug)]
struct Node {
    name: String,
    kind: Kind,
    entry: Option<usize>,
}

struct ValidatedArchive<R> {
    archive: ZipArchive<R>,
    entries: Vec<Entry>,
    nodes: BTreeMap<String, Node>,
}

/// This guard owns only a create-exclusive extraction directory, never a mount
/// point or an installed application. Keep it alive until copying has finished.
pub(super) struct ExtractedApp {
    root: PathBuf,
    pub(super) app: PathBuf,
}

impl Drop for ExtractedApp {
    fn drop(&mut self) {
        // std::fs removes symlinks themselves, not their targets. All archive
        // links have additionally been proven to remain inside this private root.
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn archive_read(reader: &mut impl Read, bytes: &mut [u8]) -> Result<()> {
    reader.read_exact(bytes).map_err(|_| "invalid_download")
}

fn seek(reader: &mut impl Seek, position: u64) -> Result<()> {
    reader
        .seek(SeekFrom::Start(position))
        .map(|_| ())
        .map_err(|_| "invalid_download")
}

fn path_key(name: &str) -> String {
    // Canonical Unicode caseless matching also catches composed/decomposed
    // aliases on Mac filesystems; ASCII lowercasing alone would miss them.
    name.nfd().default_case_fold().nfd().collect()
}

fn member_name(raw: &[u8], directory: bool) -> Result<String> {
    let raw = std::str::from_utf8(raw).map_err(|_| "invalid_download")?;
    let name = if directory {
        raw.strip_suffix('/').ok_or("invalid_download")?
    } else {
        raw
    };
    if name.is_empty()
        || name.len() >= MAX_PATH_BYTES
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '\\' | ':'))
        || name
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
        || name.split('/').next() != Some(APP)
    {
        return Err("invalid_download");
    }
    Ok(name.to_owned())
}

/// Preflight before ZipArchive::new: zip 8 deliberately indexes entries by name,
/// which would otherwise hide exact duplicates and unbounded central metadata.
fn central_directory(reader: &mut (impl Read + Seek)) -> Result<(Vec<Entry>, u64)> {
    let length = reader.seek(SeekFrom::End(0)).map_err(cache::io_error)?;
    if !(22..=MAX_BYTES).contains(&length) {
        return Err("invalid_download");
    }
    let tail_size = length.min(u16::MAX as u64 + 22) as usize;
    seek(reader, length - tail_size as u64)?;
    let mut tail = vec![0; tail_size];
    archive_read(reader, &mut tail)?;
    let end = (0..=tail.len() - 22)
        .rev()
        .find(|&at| {
            tail[at..at + 4] == *b"PK\x05\x06"
                && at + 22 + u16_at(&tail, at + 20) as usize == tail.len()
        })
        .ok_or("invalid_download")?;
    let end_position = length - tail_size as u64 + end as u64;
    // zip accepts trailing bytes after a footer. Do not let a later signature
    // inside this footer's comment choose metadata outside our bounded scan.
    if tail.windows(4).rposition(|bytes| bytes == b"PK\x05\x06") != Some(end) {
        return Err("invalid_download");
    }
    let footer = &tail[end..end + 22];
    let count = u16_at(footer, 10) as usize;
    let directory_size = u32_at(footer, 12) as u64;
    let directory_start = u32_at(footer, 16) as u64;
    if count == 0
        || count > MAX_ENTRIES
        || u16_at(footer, 4) != 0
        || u16_at(footer, 6) != 0
        || u16_at(footer, 8) as usize != count
        || directory_start + directory_size != end_position
        || directory_size < count as u64 * 46
        || directory_size > MAX_DIRECTORY_BYTES
    {
        return Err("invalid_download");
    }
    let mut entries = Vec::with_capacity(count);
    let mut names = HashSet::new();
    let mut expanded = 0u64;
    seek(reader, directory_start)?;
    for _ in 0..count {
        let mut header = [0; 46];
        archive_read(reader, &mut header)?;
        let name_len = u16_at(&header, 28) as usize;
        let extra_len = u16_at(&header, 30) as u64;
        let comment_len = u16_at(&header, 32) as u64;
        let size = u32_at(&header, 24) as u64;
        let compressed = u32_at(&header, 20) as u64;
        let local_header = u32_at(&header, 42) as u64;
        if header[..4] != *b"PK\x01\x02"
            || u16_at(&header, 8) & 0x41 != 0
            || !matches!(u16_at(&header, 10), 0 | 8)
            || u16_at(&header, 34) != 0
            || name_len == 0
            || name_len > MAX_PATH_BYTES
            || size == u32::MAX as u64
            || compressed == u32::MAX as u64
            || local_header + 30 + name_len as u64 + compressed > directory_start
        {
            return Err("invalid_download");
        }
        expanded = expanded.checked_add(size).ok_or("invalid_download")?;
        if expanded > MAX_BYTES {
            return Err("invalid_download");
        }
        let mut name = vec![0; name_len];
        archive_read(reader, &mut name)?;
        let directory = name.ends_with(b"/");
        let name = member_name(&name, directory)?;
        if !names.insert(path_key(&name)) {
            return Err("invalid_download");
        }
        let mode = u32_at(&header, 38) >> 16;
        let kind = match (mode & 0o170000, directory) {
            (0 | 0o040000, true) if size == 0 => Kind::Directory,
            (0 | 0o100000, false) => Kind::File,
            (0o120000, false) if size > 0 && size < MAX_PATH_BYTES as u64 => Kind::Link,
            _ => return Err("invalid_download"),
        };
        if mode & 0o7000 != 0 || (name == APP && kind != Kind::Directory) {
            return Err("invalid_download");
        }
        entries.push(Entry {
            name,
            kind,
            mode,
            size,
            compressed,
            header: local_header,
            link: None,
        });
        let next = reader
            .stream_position()
            .map_err(|_| "invalid_download")?
            .checked_add(extra_len + comment_len)
            .filter(|position| *position <= end_position)
            .ok_or("invalid_download")?;
        seek(reader, next)?;
    }
    if reader.stream_position().map_err(|_| "invalid_download")? != end_position {
        return Err("invalid_download");
    }
    Ok((entries, directory_start))
}

fn namespace(entries: &[Entry]) -> Result<BTreeMap<String, Node>> {
    let mut nodes: BTreeMap<String, Node> = BTreeMap::new();
    for (index, entry) in entries.iter().enumerate() {
        let parts = entry.name.split('/').collect::<Vec<_>>();
        for end in 1..=parts.len() {
            let name = parts[..end].join("/");
            let last = end == parts.len();
            let kind = if last { entry.kind } else { Kind::Directory };
            let node = nodes.entry(path_key(&name)).or_insert_with(|| Node {
                name: name.clone(),
                kind,
                entry: None,
            });
            if node.name != name || node.kind != kind {
                return Err("invalid_download");
            }
            if last {
                node.entry = Some(index);
            }
            if nodes.len() > MAX_ENTRIES {
                return Err("invalid_download");
            }
        }
    }
    if !entries.iter().any(|entry| entry.kind == Kind::File) {
        return Err("invalid_download");
    }
    Ok(nodes)
}

fn link_components(target: &str) -> Result<Vec<String>> {
    if target.is_empty()
        || target.len() >= MAX_PATH_BYTES
        || target
            .chars()
            .any(|c| c.is_control() || matches!(c, '\\' | ':'))
        || target.split('/').any(|part| part.is_empty())
    {
        return Err("invalid_download");
    }
    Ok(target.split('/').map(str::to_owned).collect())
}

fn resolve_link(
    entry: &Entry,
    entries: &[Entry],
    nodes: &BTreeMap<String, Node>,
) -> Result<String> {
    let mut resolved = entry.name.split('/').map(str::to_owned).collect::<Vec<_>>();
    resolved.pop();
    let mut pending: VecDeque<_> =
        link_components(entry.link.as_deref().ok_or("invalid_download")?)?.into();
    let mut visited = HashSet::from([path_key(&entry.name)]);
    while let Some(part) = pending.pop_front() {
        match part.as_str() {
            "." => continue,
            ".." => {
                if resolved.len() <= 1 {
                    return Err("invalid_download");
                }
                resolved.pop();
                continue;
            }
            _ => resolved.push(part),
        }
        let key = path_key(&resolved.join("/"));
        let node = nodes.get(&key).ok_or("invalid_download")?;
        if node.kind == Kind::Link {
            if !visited.insert(key) || visited.len() > MAX_SYMLINKS {
                return Err("invalid_download");
            }
            let target = entries[node.entry.ok_or("invalid_download")?]
                .link
                .as_deref()
                .ok_or("invalid_download")?;
            resolved.pop();
            let parts = link_components(target)?;
            for part in parts.into_iter().rev() {
                pending.push_front(part);
            }
        } else if node.kind == Kind::File && !pending.is_empty() {
            return Err("invalid_download");
        }
    }
    if resolved.first().map(String::as_str) != Some(APP) {
        return Err("invalid_download");
    }
    let destination = path_key(&resolved.join("/"));
    // Directory links back to an ancestor form traversal cycles even without
    // a direct link-to-link cycle (for example Resources/back -> ../..).
    if path_key(&entry.name).starts_with(&format!("{destination}/")) {
        return Err("invalid_download");
    }
    Ok(destination)
}

fn validate_links(entries: &[Entry], nodes: &BTreeMap<String, Node>) -> Result<()> {
    // Links between two sibling directory trees can form a cycle without any
    // individual link resolving cyclically. Check the complete traversal graph.
    let mut graph: BTreeMap<String, (usize, Vec<String>)> = nodes
        .iter()
        .filter(|(_, node)| node.kind == Kind::Directory)
        .map(|(key, _)| (key.clone(), (0, Vec::new())))
        .collect();
    let mut edges = Vec::new();
    for node in nodes.values().filter(|node| node.kind == Kind::Directory) {
        if let Some((parent, _)) = node.name.rsplit_once('/') {
            edges.push((path_key(parent), path_key(&node.name)));
        }
    }
    for entry in entries.iter().filter(|entry| entry.kind == Kind::Link) {
        let destination = resolve_link(entry, entries, nodes)?;
        if graph.contains_key(&destination) {
            let (parent, _) = entry.name.rsplit_once('/').ok_or("invalid_download")?;
            edges.push((path_key(parent), destination));
        }
    }
    for (parent, destination) in edges {
        graph
            .get_mut(&parent)
            .ok_or("invalid_download")?
            .1
            .push(destination.clone());
        graph.get_mut(&destination).ok_or("invalid_download")?.0 += 1;
    }
    let mut ready: VecDeque<String> = graph
        .iter()
        .filter(|(_, (degree, _))| *degree == 0)
        .map(|(key, _)| key.clone())
        .collect();
    while let Some(key) = ready.pop_front() {
        let (_, children) = graph.remove(&key).ok_or("invalid_download")?;
        for child in children {
            let (degree, _) = graph.get_mut(&child).ok_or("invalid_download")?;
            *degree -= 1;
            if *degree == 0 {
                ready.push_back(child);
            }
        }
    }
    if !graph.is_empty() {
        return Err("invalid_download");
    }
    Ok(())
}

/// Drain to EOF (including the ZIP reader's CRC check), but never allocate or
/// write bytes beyond the advertised size. A tiny compressed bomb stays bounded.
fn copy_checked(
    input: &mut impl Read,
    output: &mut impl Write,
    expected: u64,
    check_cancelled: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    let mut received = 0u64;
    let mut bytes = [0; 64 * 1024];
    loop {
        check_cancelled()?;
        let size = input.read(&mut bytes).map_err(|_| "invalid_download")?;
        if size == 0 {
            break;
        }
        received = received
            .checked_add(size as u64)
            .ok_or("invalid_download")?;
        if received > expected || received > MAX_BYTES {
            return Err("invalid_download");
        }
        output.write_all(&bytes[..size]).map_err(cache::io_error)?;
    }
    if received != expected {
        return Err("invalid_download");
    }
    Ok(())
}

fn inspect<R: Read + Seek>(
    mut reader: R,
    check_cancelled: &mut impl FnMut() -> Result<()>,
) -> Result<ValidatedArchive<R>> {
    let (mut entries, directory_start) = central_directory(&mut reader)?;
    let nodes = namespace(&entries)?;
    seek(&mut reader, 0)?;
    let mut archive = ZipArchive::new(reader).map_err(|_| "invalid_download")?;
    if archive.len() != entries.len()
        || archive.offset() != 0
        || archive.central_directory_start() != directory_start
    {
        return Err("invalid_download");
    }
    for (index, entry) in entries.iter_mut().enumerate() {
        check_cancelled()?;
        let mut member = archive.by_index(index).map_err(|_| "invalid_download")?;
        if member_name(member.name_raw(), member.is_dir())? != entry.name
            || member.size() != entry.size
            || member.compressed_size() != entry.compressed
            || member.header_start() != entry.header
            || member.encrypted()
            || !matches!(
                member.compression(),
                CompressionMethod::Stored | CompressionMethod::Deflated
            )
            || member
                .data_start()
                .and_then(|position| position.checked_add(entry.compressed))
                .is_none_or(|end| end > directory_start)
        {
            return Err("invalid_download");
        }
        if entry.kind == Kind::Link {
            let mut target = Vec::with_capacity(entry.size as usize);
            copy_checked(&mut member, &mut target, entry.size, check_cancelled)?;
            let target = String::from_utf8(target).map_err(|_| "invalid_download")?;
            link_components(&target)?;
            entry.link = Some(target);
        } else {
            // No staging exists until every member passes size/CRC validation.
            copy_checked(
                &mut member,
                &mut std::io::sink(),
                entry.size,
                check_cancelled,
            )?;
        }
    }
    validate_links(&entries, &nodes)?;
    Ok(ValidatedArchive {
        archive,
        entries,
        nodes,
    })
}

#[cfg(unix)]
fn create_file(path: &Path) -> Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    #[cfg(target_os = "macos")]
    options.custom_flags(0x100); // O_NOFOLLOW, Darwin SDK.
    #[cfg(target_os = "linux")]
    options.custom_flags(0x20000);
    options.open(path).map_err(cache::io_error)
}

#[cfg(unix)]
fn extract_reader<R: Read + Seek>(
    reader: R,
    parent: &Path,
    mut check_cancelled: impl FnMut() -> Result<()>,
) -> Result<ExtractedApp> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let ValidatedArchive {
        mut archive,
        entries,
        nodes,
    } = inspect(reader, &mut check_cancelled)?;
    check_cancelled()?;
    let root = cache::unique_dir(parent)?;
    let extracted = ExtractedApp {
        app: root.join(APP),
        root,
    };
    // BTreeMap lexical ordering visits every parent before its descendants.
    for node in nodes.values().filter(|node| node.kind == Kind::Directory) {
        check_cancelled()?;
        fs::DirBuilder::new()
            .mode(0o700)
            .create(extracted.root.join(&node.name))
            .map_err(cache::io_error)?;
    }
    for (index, entry) in entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.kind == Kind::File)
    {
        check_cancelled()?;
        let path = extracted.root.join(&entry.name);
        let mut output = create_file(&path)?;
        let mut member = archive.by_index(index).map_err(|_| "invalid_download")?;
        copy_checked(&mut member, &mut output, entry.size, &mut check_cancelled)?;
        // Keep owner access for cleanup, preserve all executable bits, never
        // propagate setuid/setgid/sticky bits (rejected during preflight).
        output
            .set_permissions(fs::Permissions::from_mode(0o600 | (entry.mode & 0o777)))
            .map_err(cache::io_error)?;
        // This directory is throwaway staging, never a resumable generation.
        // Close each file before native verification; per-file durable flushes
        // would add thousands of serial I/O waits without a recovery benefit.
        drop(output);
    }
    for entry in entries.iter().filter(|entry| entry.kind == Kind::Link) {
        check_cancelled()?;
        std::os::unix::fs::symlink(
            entry.link.as_deref().ok_or("invalid_download")?,
            extracted.root.join(&entry.name),
        )
        .map_err(cache::io_error)?;
    }
    Ok(extracted)
}

#[cfg(target_os = "macos")]
pub(super) fn extract(
    file: &Path,
    parent: &Path,
    check_cancelled: impl FnMut() -> Result<()>,
) -> Result<ExtractedApp> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let reader = OpenOptions::new()
        .read(true)
        .custom_flags(0x100)
        .open(file)
        .map_err(cache::io_error)?;
    let metadata = reader.metadata().map_err(cache::io_error)?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err("unsafe_cache");
    }
    extract_reader(reader, parent, check_cancelled)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{io::Cursor, os::unix::fs::PermissionsExt};
    use zip::{write::SimpleFileOptions, ZipWriter};

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            Self(cache::unique_dir(&std::env::temp_dir()).unwrap())
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn archive(entries: &[(&str, Kind, &[u8])]) -> Vec<u8> {
        compressed_archive(entries, CompressionMethod::Stored)
    }

    fn compressed_archive(
        entries: &[(&str, Kind, &[u8])],
        compression: CompressionMethod,
    ) -> Vec<u8> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, kind, data) in entries {
            let options = SimpleFileOptions::default()
                .compression_method(compression)
                .unix_permissions(0o755);
            match kind {
                Kind::Directory => zip.add_directory(*name, options).unwrap(),
                Kind::Link => zip
                    .add_symlink(name, std::str::from_utf8(data).unwrap(), options)
                    .unwrap(),
                Kind::File => {
                    zip.start_file(name, options).unwrap();
                    zip.write_all(data).unwrap();
                }
            }
        }
        zip.finish().unwrap().into_inner()
    }

    fn headers(bytes: &[u8]) -> Vec<usize> {
        let footer = bytes.len() - 22;
        let mut position = u32_at(bytes, footer + 16) as usize;
        (0..u16_at(bytes, footer + 10))
            .map(|_| {
                let start = position;
                position += 46
                    + u16_at(bytes, position + 28) as usize
                    + u16_at(bytes, position + 30) as usize
                    + u16_at(bytes, position + 32) as usize;
                start
            })
            .collect()
    }

    fn rejected(bytes: Vec<u8>) {
        let parent = Temp::new();
        assert!(extract_reader(Cursor::new(bytes), &parent.0, || Ok(())).is_err());
        assert_eq!(
            fs::read_dir(&parent.0).unwrap().count(),
            0,
            "invalid archives must leave no staging"
        );
    }

    #[test]
    fn installer_zip_rejects_ambiguous_footer_before_library_allocation() {
        let original = archive(&[("Claude.app/file", Kind::File, b"fixture")]);
        let footer = original.len() - 22;
        let mut ordinary_comment = original.clone();
        ordinary_comment[footer + 20..footer + 22].copy_from_slice(&3u16.to_le_bytes());
        ordinary_comment.extend_from_slice(b"ok!");
        assert!(central_directory(&mut Cursor::new(ordinary_comment)).is_ok());

        // The ZIP library permits trailing bytes after an EOCD. A second
        // footer inside the real comment must not select different metadata
        // after our size/count preflight, even when it advertises zero files.
        for mut comment in [b"PK\x05\x06".to_vec(), vec![0; 22]] {
            comment[..4].copy_from_slice(b"PK\x05\x06");
            comment.push(b'x');
            let mut ambiguous = original.clone();
            ambiguous[footer + 20..footer + 22]
                .copy_from_slice(&(comment.len() as u16).to_le_bytes());
            ambiguous.extend_from_slice(&comment);
            assert!(central_directory(&mut Cursor::new(ambiguous.clone())).is_err());
            rejected(ambiguous);
        }
    }

    #[test]
    fn installer_zip_valid_framework_links_preserve_modes_and_cleanup() {
        let bytes = archive(&[
            ("Claude.app/", Kind::Directory, b""),
            (
                "Claude.app/Contents/MacOS/Claude",
                Kind::File,
                b"fixture, never executed",
            ),
            (
                "Claude.app/Contents/Frameworks/Test.framework/Versions/A/Test",
                Kind::File,
                b"binary",
            ),
            (
                "Claude.app/Contents/Frameworks/Test.framework/Versions/A/Resources/info",
                Kind::File,
                b"resource",
            ),
            (
                "Claude.app/Contents/Frameworks/Test.framework/Versions/Current",
                Kind::Link,
                b"A",
            ),
            (
                "Claude.app/Contents/Frameworks/Test.framework/Test",
                Kind::Link,
                b"Versions/Current/Test",
            ),
            (
                "Claude.app/Contents/Frameworks/Test.framework/Resources",
                Kind::Link,
                b"Versions/Current/Resources",
            ),
        ]);
        let parent = Temp::new();
        fs::write(parent.0.join("unrelated"), b"keep").unwrap();
        let extracted = extract_reader(Cursor::new(bytes), &parent.0, || Ok(())).unwrap();
        let root = extracted.root.clone();
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let executable = extracted.app.join("Contents/MacOS/Claude");
        assert_eq!(
            fs::metadata(executable).unwrap().permissions().mode() & 0o111,
            0o111
        );
        let resource = extracted
            .app
            .join("Contents/Frameworks/Test.framework/Resources");
        assert!(fs::symlink_metadata(&resource)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read(resource.join("info")).unwrap(), b"resource");
        drop(extracted);
        assert!(!root.exists());
        assert_eq!(fs::read(parent.0.join("unrelated")).unwrap(), b"keep");
    }

    #[test]
    fn installer_zip_rejects_unsafe_and_normalization_duplicate_members() {
        for name in [
            "/Claude.app/file",
            "../Claude.app/file",
            "Claude.app/../file",
            "Claude.app/./file",
            "Claude.app//file",
            "Claude.app\\file",
            "Claude.app/file\0suffix",
            "Claude.app/C:/file",
            "Other.app/file",
            "__MACOSX/Claude.app/file",
        ] {
            rejected(archive(&[(name, Kind::File, b"a")]));
        }
        for (first, second) in [
            ("Claude.app/file", "Claude.app/FILE"),
            ("Claude.app/café", "Claude.app/cafe\u{301}"),
            ("Claude.app/straße", "Claude.app/STRASSE"),
            ("Claude.app/A/file", "Claude.app/a/other"),
        ] {
            rejected(archive(&[
                (first, Kind::File, b"a"),
                (second, Kind::File, b"b"),
            ]));
        }
        // The library itself collapses duplicate raw central names. Mutate a
        // second name so our preflight, not ZipArchive::len(), must reject it.
        let mut duplicate = archive(&[
            ("Claude.app/a", Kind::File, b"a"),
            ("Claude.app/b", Kind::File, b"b"),
        ]);
        let second = headers(&duplicate)[1];
        duplicate[second + 46 + "Claude.app/".len()] = b'a';
        rejected(duplicate);
    }

    #[test]
    fn installer_zip_rejects_link_escape_parent_collisions_and_cycles() {
        for target in [
            "/tmp/file",
            "../../outside",
            "missing",
            "link",
            ".",
            "a\\b",
            "x\0y",
        ] {
            rejected(archive(&[
                ("Claude.app/contents", Kind::File, b"a"),
                ("Claude.app/link", Kind::Link, target.as_bytes()),
            ]));
        }
        rejected(archive(&[
            ("Claude.app/file", Kind::File, b"a"),
            ("Claude.app/link", Kind::Link, b"next"),
            ("Claude.app/next", Kind::Link, b"link"),
        ]));
        for kind in [Kind::Link, Kind::File] {
            rejected(archive(&[
                ("Claude.app/real/child", Kind::File, b"a"),
                ("Claude.app/alias", kind, b"real"),
                ("Claude.app/alias/child", Kind::File, b"b"),
            ]));
        }
        rejected(archive(&[
            ("Claude.app/A/file", Kind::File, b"a"),
            ("Claude.app/B/file", Kind::File, b"b"),
            ("Claude.app/A/link", Kind::Link, b"../B"),
            ("Claude.app/B/link", Kind::Link, b"../A"),
        ]));
    }

    #[test]
    fn installer_zip_rejects_special_modes_crc_errors_and_expansion_bombs() {
        let original = archive(&[
            ("Claude.app/a", Kind::File, b"123456789"),
            ("Claude.app/b", Kind::File, b"b"),
        ]);
        let offsets = headers(&original);
        for mode in [
            0o010644u32,
            0o020600,
            0o060600,
            0o140600,
            0o104755,
            0o102755,
        ] {
            let mut bytes = original.clone();
            bytes[offsets[0] + 38..offsets[0] + 42].copy_from_slice(&(mode << 16).to_le_bytes());
            rejected(bytes);
        }
        let mut bomb = original.clone();
        for offset in &offsets {
            bomb[offset + 24..offset + 28].copy_from_slice(&3_000_000_000u32.to_le_bytes());
        }
        rejected(bomb);
        let mut oversized_actual = original.clone();
        oversized_actual[offsets[0] + 24..offsets[0] + 28].copy_from_slice(&1u32.to_le_bytes());
        rejected(oversized_actual);
        let mut wrong_crc = original.clone();
        let data = 30 + u16_at(&wrong_crc, 26) as usize + u16_at(&wrong_crc, 28) as usize;
        wrong_crc[data] ^= 1;
        rejected(wrong_crc);
        let mut encrypted = original.clone();
        encrypted[offsets[0] + 8] |= 1;
        rejected(encrypted);
        let mut zip64 = original;
        let end = zip64.len() - 22;
        zip64[end + 10..end + 12].copy_from_slice(&u16::MAX.to_le_bytes());
        rejected(zip64);
    }

    #[test]
    fn installer_zip_deflate_stream_is_size_and_crc_bounded() {
        let payload = vec![0x41; 256 * 1024];
        let original = compressed_archive(
            &[("Claude.app/Contents/file", Kind::File, &payload)],
            CompressionMethod::Deflated,
        );
        assert!(original.len() < payload.len() / 10);
        let parent = Temp::new();
        let extracted =
            extract_reader(Cursor::new(original.clone()), &parent.0, || Ok(())).unwrap();
        assert_eq!(
            fs::read(extracted.app.join("Contents/file")).unwrap(),
            payload
        );
        drop(extracted);
        let header = headers(&original)[0];
        let mut bomb = original;
        bomb[header + 24..header + 28].copy_from_slice(&1u32.to_le_bytes());
        rejected(bomb);
    }

    #[test]
    fn installer_zip_cancellation_and_create_new_preserve_other_files() {
        let parent = Temp::new();
        let existing = parent.0.join("existing");
        fs::write(&existing, b"keep").unwrap();
        assert!(create_file(&existing).is_err());
        let alias = parent.0.join("alias");
        std::os::unix::fs::symlink(&existing, &alias).unwrap();
        assert!(create_file(&alias).is_err());
        let bytes = archive(&[("Claude.app/Contents/file", Kind::File, b"payload")]);
        let result = extract_reader(Cursor::new(bytes), &parent.0, || {
            if fs::read_dir(&parent.0).unwrap().count() > 2 {
                Err("cancelled")
            } else {
                Ok(())
            }
        });
        assert!(matches!(result, Err("cancelled")));
        assert_eq!(fs::read_dir(&parent.0).unwrap().count(), 2);
        assert_eq!(fs::read(&existing).unwrap(), b"keep");
    }
}
