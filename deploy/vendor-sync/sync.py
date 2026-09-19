#!/usr/bin/env python3
"""Private official-desktop staging. No publish, install, login or signing operations."""
from __future__ import annotations

import argparse
import contextlib
import hashlib
import http.client
import ipaddress
import json
import os
from pathlib import Path, PurePosixPath
import plistlib
import re
import shutil
import socket
import ssl
import stat
import sys
import tempfile
import time
import unicodedata
from datetime import datetime, timezone
from urllib.parse import urljoin, urlsplit
import xml.etree.ElementTree as ET
import zipfile

CHUNK = 64 * 1024
MAX_BYTES = 4 * 1024 * 1024 * 1024
MAX_JSON_BYTES = 1024 * 1024
MAX_ZIP_ENTRIES = 65534  # Classic ZIP; 65535 is the ZIP64 sentinel.
MAX_PATH_BYTES = 1024  # Darwin PATH_MAX, including the terminating NUL.
MAX_DIRECTORY_BYTES = MAX_ZIP_ENTRIES * (MAX_PATH_BYTES + 46)
MAX_SYMLINKS = 32  # Darwin MAXSYMLINKS.
HOSTS = {"codex": {"persistent.oaistatic.com"}, "claude": {"claude.ai", "downloads.claude.ai"}}
CONTENT_TYPES = {"application/octet-stream", "application/x-apple-diskimage", "application/vnd.ms-appx", "application/msix", "application/zip"}
SOURCE_FILE = Path(__file__).with_name("sources.json")


class SyncError(Exception):
    """A redacted machine-readable error; never include HTTP body/cookies."""


def utc() -> str:
    return datetime.now(timezone.utc).isoformat()


def digest(path: Path) -> str:
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(CHUNK), b""):
            result.update(chunk)
    return result.hexdigest()


def regular(path: Path) -> None:
    if path.is_symlink() or (path.exists() and not stat.S_ISREG(path.stat().st_mode)):
        raise SyncError("unsafe_file")


def child(root: Path, relative: str) -> Path:
    parts = PurePosixPath(relative).parts
    if not parts or PurePosixPath(relative).is_absolute() or any(p in {".", ".."} for p in parts):
        raise SyncError("unsafe_path")
    result = root
    for part in parts:
        result = result / part
        if result.is_symlink():
            raise SyncError("unsafe_symlink")
    return result


def private_root(path: Path, create: bool = True) -> Path:
    path = path.absolute()
    # DynamicUser's StateDirectory symlink is systemd-owned, not source input.
    if path.is_symlink():
        if str(path) != "/var/lib/yeschoy-vendor-sync" or str(path.resolve()) != "/var/lib/private/yeschoy-vendor-sync":
            raise SyncError("unsafe_state_root")
    path = path.resolve()
    if str(path) in {"/", "/var", "/var/lib", str(Path.home())} or "/public" in str(path):
        raise SyncError("unsafe_state_root")
    if not path.exists() and not create:
        return path
    if create:
        path.mkdir(mode=0o700, parents=True, exist_ok=True)
    if not path.is_dir() or path.stat().st_mode & 0o077:
        raise SyncError("state_directory_must_be_private")
    return path


def atomic_json(path: Path, value: dict) -> None:
    regular(path)
    fd, name = tempfile.mkstemp(prefix=".receipt-", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as stream:
            json.dump(value, stream, ensure_ascii=False, sort_keys=True, indent=2)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(name, path)
        fd = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
    finally:
        if os.path.exists(name):
            os.unlink(name)  # Only this call's exclusive temporary receipt.


def read_json(path: Path, default: dict) -> dict:
    regular(path)
    if not path.exists():
        return default
    if path.stat().st_size > 2 * 1024 * 1024:
        raise SyncError("state_file_too_large")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
        if not isinstance(value, dict):
            raise ValueError()
        return value
    except (ValueError, UnicodeError) as exc:
        raise SyncError("corrupt_state") from exc


ARTIFACT_OBJECT = re.compile(r"(?P<sha256>[a-f0-9]{64})\.(?P<format>msix|dmg|zip)")
ARTIFACT_RECEIPT = re.compile(r"[a-f0-9]{64}\.json")


def retained_candidate_names(catalog: dict) -> set[str]:
    rows = catalog.get("sources", {})
    if catalog.get("schemaVersion") != 1 or not isinstance(rows, dict):
        raise SyncError("corrupt_catalog")
    retained = set()
    for row in rows.values():
        if not isinstance(row, dict):
            raise SyncError("corrupt_catalog")
        for key in ("candidate", "previousCandidate"):
            entry = row.get(key)
            if not entry:
                continue
            if not isinstance(entry, dict):
                raise SyncError("corrupt_catalog")
            relative = entry.get("path", "")
            match = re.fullmatch(r"artifacts/([a-f0-9]{64})\.(msix|dmg|zip)", relative)
            if match is None or entry.get("sha256") != match.group(1):
                raise SyncError("invalid_candidate_path")
            retained.add(Path(relative).name)
            retained.add(f"{match.group(1)}.json")
    return retained


def prune_private_cache(root: Path, catalog: dict) -> dict:
    """Keep each source's current candidate and one previous candidate."""
    directory = child(root, "artifacts")
    if not directory.exists():
        return {"status": "pruned", "filesDeleted": 0, "bytesDeleted": 0}
    if not directory.is_dir():
        raise SyncError("unsafe_file")
    retained = retained_candidate_names(catalog)
    obsolete_receipts = []
    obsolete_objects = []
    for path in sorted(directory.iterdir()):
        regular(path)
        if path.name in retained:
            continue
        if ARTIFACT_RECEIPT.fullmatch(path.name):
            obsolete_receipts.append(path)
        elif ARTIFACT_OBJECT.fullmatch(path.name):
            obsolete_objects.append(path)
    deleted = 0
    deleted_bytes = 0
    for path in (*obsolete_receipts, *obsolete_objects):
        deleted_bytes += path.stat().st_size
        path.unlink()
        deleted += 1
    if deleted:
        descriptor = os.open(directory, os.O_RDONLY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
    return {"status": "pruned", "filesDeleted": deleted, "bytesDeleted": deleted_bytes}


@contextlib.contextmanager
def locked(root: Path):
    import fcntl  # Server lock; native verification helper also runs on Windows.
    path = child(root, ".sync.lock")
    regular(path)
    fd = os.open(path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    try:
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            raise SyncError("already_running") from exc
        yield
    finally:
        os.close(fd)


def validate_url(source: dict, url: str):
    if not isinstance(url, str) or len(url) > 4096 or any(ord(char) <= 32 or ord(char) == 127 for char in url):
        raise SyncError("unsafe_url")
    try:
        parsed = urlsplit(url)
        valid = (parsed.scheme == "https" and parsed.hostname in HOSTS[source["app"]]
                 and parsed.port in {None, 443} and not parsed.username and not parsed.password
                 and not parsed.fragment and not parsed.query)
    except (ValueError, KeyError) as exc:
        raise SyncError("unsafe_url") from exc
    if not valid:
        raise SyncError("unsafe_url")
    path = parsed.path
    if parsed.hostname == "persistent.oaistatic.com":
        valid = path.startswith("/codex-app-prod/")
    elif parsed.hostname == "claude.ai":
        valid = url == source["url"]
    else:
        platform = "darwin" if source["platform"] == "macos" else "win32"
        if source.get("resolver") == "release_feed":
            valid = url == source["url"] or re.fullmatch(r"/releases/darwin/universal/[0-9]+(?:\.[0-9]+){1,3}/Claude-[a-f0-9]{40}\.zip", path) is not None
        else:
            valid = path.startswith(f"/releases/{platform}/{source['architecture']}/")
    if not valid or "%" in path or ".." in path:
        raise SyncError("unsafe_url_path")
    return parsed


def public_addresses(host: str) -> list[str]:
    addresses = list(dict.fromkeys(item[4][0] for item in socket.getaddrinfo(host, 443, type=socket.SOCK_STREAM)))
    if not addresses or any(not ipaddress.ip_address(address).is_global or ipaddress.ip_address(address).is_multicast for address in addresses):
        raise SyncError("non_public_address")
    return addresses


class PinnedHTTPS(http.client.HTTPSConnection):
    def __init__(self, host: str, addresses: list[str]):
        super().__init__(host, timeout=30, context=ssl.create_default_context())
        self.addresses = addresses

    def connect(self):
        last = None
        for address in self.addresses:
            sock = None
            try:
                sock = socket.create_connection((address, 443), self.timeout)
                self.sock = self._context.wrap_socket(sock, server_hostname=self.host)
                return
            except OSError as exc:
                if sock:
                    sock.close()
                last = exc
        raise SyncError("connection_failed") from last


class Transport:
    @contextlib.contextmanager
    def request(self, source: dict, method: str, url: str, headers: dict | None = None):
        parsed = validate_url(source, url)
        connection = PinnedHTTPS(parsed.hostname, public_addresses(parsed.hostname))
        try:
            connection.request(method, parsed.path, headers={"User-Agent": "Yecai-Vendor-Sync/1.0", "Accept-Encoding": "identity", **(headers or {})})
            response = connection.getresponse()
            yield response
        except (OSError, http.client.HTTPException) as exc:
            raise SyncError("network_error") from exc
        finally:
            connection.close()


def headers_of(response) -> dict:
    return {key.lower(): value for key, value in response.getheaders()}


def positive_length(value: str) -> int:
    if not re.fullmatch(r"[0-9]{1,15}", value) or not 0 < int(value) <= MAX_BYTES:
        raise SyncError("missing_or_invalid_length")
    return int(value)


def check_type(headers: dict) -> None:
    if headers.get("content-type", "").split(";", 1)[0].strip().lower() not in CONTENT_TYPES:
        raise SyncError("unexpected_content_type")
    if headers.get("content-encoding", "identity") != "identity" or headers.get("transfer-encoding"):
        raise SyncError("unexpected_encoding")


def strict_json(data: bytes) -> dict:
    def pairs(items):
        value = {}
        for key, item in items:
            if key in value:
                raise ValueError("duplicate_key")
            value[key] = item
        return value
    try:
        if len(data) > MAX_JSON_BYTES:
            raise ValueError("oversized_json")
        value = json.loads(data.decode("utf-8"), object_pairs_hook=pairs,
                           parse_constant=lambda _: (_ for _ in ()).throw(ValueError("invalid_number")))
        if not isinstance(value, dict):
            raise ValueError("invalid_json_object")
        return value
    except (ValueError, UnicodeError, RecursionError) as exc:
        raise SyncError("invalid_json") from exc


def version_tuple(value: str) -> tuple[int, ...]:
    if not isinstance(value, str) or len(value) > 64 or not re.fullmatch(r"[0-9]+(?:\.[0-9]+){1,3}", value):
        raise SyncError("invalid_version_metadata")
    parts = tuple(map(int, value.split(".")))
    return parts + (0,) * (4 - len(parts))


def release_feed(source: dict, transport: Transport) -> tuple[str, str]:
    with transport.request(source, "GET", source["url"]) as response:
        headers = headers_of(response)
        if response.status != 200 or headers.get("cf-mitigated"):
            raise SyncError("source_challenge" if headers.get("cf-mitigated") else f"http_{response.status}")
        if headers.get("content-type", "").split(";", 1)[0].strip().lower() != "application/json" or headers.get("content-encoding", "identity") != "identity":
            raise SyncError("unexpected_feed_type")
        length = headers.get("content-length")
        if length is not None and (not re.fullmatch(r"[0-9]{1,10}", length) or not 0 < int(length) <= MAX_JSON_BYTES):
            raise SyncError("feed_too_large")
        body = response.read(MAX_JSON_BYTES + 1)
        if len(body) > MAX_JSON_BYTES:
            raise SyncError("feed_too_large")
        if length is not None and len(body) != int(length):
            raise SyncError("incomplete_feed")
    feed = strict_json(body)
    version = feed.get("currentRelease")
    version_tuple(version)
    releases = feed.get("releases")
    if not isinstance(releases, list) or not all(isinstance(item, dict) for item in releases):
        raise SyncError("invalid_release_feed")
    matches = [item for item in releases if item.get("version") == version]
    if len(matches) != 1:
        raise SyncError("ambiguous_current_release")
    update = matches[0].get("updateTo")
    if not isinstance(update, dict) or update.get("version") != version:
        raise SyncError("invalid_release_feed")
    url = update.get("url")
    parsed = validate_url(source, url)
    if not parsed.path.startswith(f"/releases/darwin/universal/{version}/") or not parsed.path.endswith(".zip"):
        raise SyncError("release_version_mismatch")
    return url, version


def discover(source: dict, transport: Transport) -> dict:
    feed_version = None
    url = source["url"]
    method = "GET" if source["resolver"] else "HEAD"
    if source.get("resolver") == "release_feed":
        url, feed_version = release_feed(source, transport)
        method = "HEAD"
    for _ in range(6):
        validate_url(source, url)
        with transport.request(source, method, url) as response:
            headers = headers_of(response)
            if response.status in {301, 302, 303, 307, 308}:
                url = urljoin(url, headers.get("location", ""))
                validate_url(source, url)
                if feed_version and not urlsplit(url).path.startswith(f"/releases/darwin/universal/{feed_version}/"):
                    raise SyncError("release_version_mismatch")
                method = "HEAD"
                continue
            if response.status != 200:
                raise SyncError("source_challenge" if headers.get("cf-mitigated") == "challenge" else f"http_{response.status}")
            if not urlsplit(url).path.lower().endswith("." + source["format"]):
                raise SyncError("not_an_installer_url")
            check_type(headers)
            size = positive_length(headers.get("content-length", ""))
            etag = headers.get("etag", "")
            modified = headers.get("last-modified", "")
            if not etag or etag.startswith("W/") or not re.fullmatch(r'"[^"\r\n]{1,200}"', etag):
                raise SyncError("strong_validator_required")
            version = feed_version or headers.get("x-ms-meta-package_version", "unknown")
            if version != "unknown" and not re.fullmatch(r"\d+(?:\.\d+){1,3}", version):
                raise SyncError("invalid_version_metadata")
            metadata = {"url": url, "size": size, "etag": etag, "lastModified": modified, "versionHint": version}
            metadata["fingerprint"] = hashlib.sha256(json.dumps(metadata, sort_keys=True).encode()).hexdigest()
            return metadata
    raise SyncError("too_many_redirects")


def preflight_macos_zip(stream) -> None:
    """Bound classic ZIP metadata before ZipFile eagerly reads its directory."""
    def number(data, at, length):
        return int.from_bytes(data[at:at + length], "little")
    stream.seek(0, os.SEEK_END)
    length = stream.tell()
    if not 22 <= length <= MAX_BYTES:
        raise SyncError("invalid_zip_size")
    tail_size = min(length, 65535 + 22)
    stream.seek(length - tail_size)
    tail = stream.read(tail_size)
    end = next((offset for offset in range(len(tail) - 22, -1, -1)
                if tail[offset:offset + 4] == b"PK\x05\x06" and offset + 22 + number(tail, offset + 20, 2) == len(tail)), None)
    # Python zipfile selects the final signature even when it occurs inside
    # another EOCD's comment. Require parser agreement before the library can
    # use different (potentially unbounded) central-directory metadata.
    if end is None or end != tail.rfind(b"PK\x05\x06"):
        raise SyncError("invalid_zip_directory")
    footer = tail[end:end + 22]
    end_position = length - tail_size + end
    count, size, start = number(footer, 10, 2), number(footer, 12, 4), number(footer, 16, 4)
    if not 0 < count <= MAX_ZIP_ENTRIES or number(footer, 4, 2) != 0 or number(footer, 6, 2) != 0 or number(footer, 8, 2) != count or start + size != end_position or not count * 46 <= size <= MAX_DIRECTORY_BYTES:
        raise SyncError("invalid_zip_directory")
    stream.seek(0)
    if stream.read(4) != b"PK\x03\x04":
        raise SyncError("invalid_zip_header")
    stream.seek(start)
    expanded = 0
    for _ in range(count):
        if stream.tell() + 46 > end_position:
            raise SyncError("invalid_zip_directory")
        header = stream.read(46)
        if len(header) != 46:
            raise SyncError("invalid_zip_directory")
        name_len, extra_len, comment_len = number(header, 28, 2), number(header, 30, 2), number(header, 32, 2)
        compressed, size, local = number(header, 20, 4), number(header, 24, 4), number(header, 42, 4)
        if header[:4] != b"PK\x01\x02" or number(header, 8, 2) & 0x41 or number(header, 10, 2) not in {0, 8} or number(header, 34, 2) != 0 or not 0 < name_len <= MAX_PATH_BYTES or size == 0xffffffff or compressed == 0xffffffff or local + 30 + name_len + compressed > start:
            raise SyncError("invalid_zip_directory")
        if stream.tell() + name_len + extra_len + comment_len > end_position:
            raise SyncError("invalid_zip_directory")
        name = stream.read(name_len)
        if len(name.rstrip(b"/")) >= MAX_PATH_BYTES:
            raise SyncError("zip_path_limit")
        mode = number(header, 38, 4) >> 16
        if stat.S_ISLNK(mode) and not 0 < size < MAX_PATH_BYTES:
            raise SyncError("zip_link_limit")
        expanded += size
        if expanded > MAX_BYTES:
            raise SyncError("zip_expansion_limit")
        stream.seek(extra_len + comment_len, os.SEEK_CUR)
    if stream.tell() != end_position:
        raise SyncError("invalid_zip_directory")
    stream.seek(0)


@contextlib.contextmanager
def mac_zip(path: Path):
    regular(path)
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(fd, "rb") as stream:
        preflight_macos_zip(stream)
        with zipfile.ZipFile(stream) as package:
            yield package


def mac_zip_members(package: zipfile.ZipFile) -> tuple[list, dict]:
    """Validate all entries before writing; framework links are created last."""
    entries = package.infolist()
    if not entries or len(entries) > MAX_ZIP_ENTRIES:
        raise SyncError("zip_entry_limit")
    paths, folded, links = {}, set(), {}
    def path_key(value):
        return unicodedata.normalize("NFD", unicodedata.normalize("NFD", value).casefold())
    expanded = 0
    for info in entries:
        name = info.filename.rstrip("/")
        if len(name.encode("utf-8")) >= MAX_PATH_BYTES:
            raise SyncError("zip_path_limit")
        parts = PurePosixPath(name).parts
        if not parts or parts[0] != "Claude.app" or info.filename.startswith("/") or "\\" in name or ":" in name or any(ord(c) < 32 or ord(c) == 127 for c in name) or any(p in {".", ".."} for p in parts) or str(PurePosixPath(name)) != name:
            raise SyncError("unsafe_zip_path")
        key = path_key(name)
        if key in folded:
            raise SyncError("ambiguous_archive")
        folded.add(key)
        mode = info.external_attr >> 16
        kind = stat.S_IFMT(mode)
        directory = info.is_dir()
        if kind not in {0, stat.S_IFREG, stat.S_IFDIR, stat.S_IFLNK} or (kind == stat.S_IFDIR) != directory and kind != 0 or mode & 0o7000 or (name == "Claude.app" and not directory) or info.flag_bits & 1 or info.compress_type not in {zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED}:
            raise SyncError("unsafe_zip_entry")
        expanded += info.file_size
        if info.file_size < 0 or expanded > MAX_BYTES or directory and info.file_size:
            raise SyncError("zip_expansion_limit")
        paths[name] = (info, "directory" if directory else "link" if kind == stat.S_IFLNK else "file")
        if kind == stat.S_IFLNK:
            if not 0 < info.file_size < MAX_PATH_BYTES:
                raise SyncError("unsafe_zip_link")
            try:
                target = package.read(info).decode("utf-8")
            except (UnicodeError, ValueError) as exc:
                raise SyncError("unsafe_zip_link") from exc
            if not target or any(not part for part in target.split("/")) or "\\" in target or ":" in target or any(ord(c) < 32 or ord(c) == 127 for c in target):
                raise SyncError("unsafe_zip_link")
            links[name] = target
    # Include implicit parents in the same canonical caseless namespace as
    # explicit members. A directory cannot alias another spelling, file or link.
    nodes = {}
    for name, (_, kind) in paths.items():
        parts = name.split("/")
        for end in range(1, len(parts) + 1):
            prefix = "/".join(parts[:end])
            entry = (prefix, kind if end == len(parts) else "directory")
            key = path_key(prefix)
            if key in nodes and nodes[key] != entry:
                raise SyncError("zip_namespace_conflict")
            nodes[key] = entry
            if len(nodes) > MAX_ZIP_ENTRIES:
                raise SyncError("zip_entry_limit")
    # Ordinary directory containment plus directory links must form a DAG.
    # Link-to-link checks alone miss A/link -> ../B, B/link -> ../A cycles.
    graph = {key: [] for key, (_, kind) in nodes.items() if kind == "directory"}
    for key, (name, kind) in nodes.items():
        if kind == "directory" and "/" in name:
            graph[path_key(name.rsplit("/", 1)[0])].append(key)
    for name, target in links.items():
        pending = target.split("/")
        resolved = list(PurePosixPath(name).parent.parts)
        visited = {path_key(name)}
        while pending:
            piece = pending.pop(0)
            if piece in {"", "."}:
                continue
            if piece == "..":
                if len(resolved) <= 1:
                    raise SyncError("unsafe_zip_link")
                resolved.pop()
                continue
            resolved.append(piece)
            current = path_key("/".join(resolved))
            node = nodes.get(current)
            if node is None:
                raise SyncError("unsafe_zip_link")
            if node[1] == "link":
                if current in visited or len(visited) >= MAX_SYMLINKS:
                    raise SyncError("cyclic_zip_link")
                visited.add(current)
                resolved.pop()
                pending = links[node[0]].split("/") + pending
            elif node[1] == "file" and pending:
                raise SyncError("unsafe_zip_link")
        destination = path_key("/".join(resolved))
        if not resolved or resolved[0] != "Claude.app" or destination not in nodes:
            raise SyncError("unsafe_zip_link")
        if path_key(name).startswith(destination + "/"):
            raise SyncError("cyclic_zip_link")
        if destination in graph:
            graph[path_key(name.rsplit("/", 1)[0])].append(destination)
    degrees = dict.fromkeys(graph, 0)
    for children in graph.values():
        for child_name in children:
            degrees[child_name] += 1
    ready = [name for name, degree in degrees.items() if degree == 0]
    seen = 0
    while ready:
        seen += 1
        for name in graph[ready.pop()]:
            degrees[name] -= 1
            if degrees[name] == 0:
                ready.append(name)
    if seen != len(graph):
        raise SyncError("cyclic_zip_link")
    plist_name = "Claude.app/Contents/Info.plist"
    if plist_name not in paths or paths[plist_name][1] != "file" or paths[plist_name][0].file_size > MAX_JSON_BYTES:
        raise SyncError("missing_zip_identity")
    try:
        info = plistlib.loads(package.read(paths[plist_name][0]))
    except (ValueError, plistlib.InvalidFileException, OverflowError, RecursionError) as exc:
        raise SyncError("invalid_zip_identity") from exc
    if not isinstance(info, dict):
        raise SyncError("invalid_zip_identity")
    return list(paths.values()), info


def extract_macos_zip(path: Path, destination: Path) -> Path:
    regular(path)
    if destination.is_symlink() or not destination.is_dir() or any(destination.iterdir()) or destination.stat().st_uid != os.geteuid() or destination.stat().st_mode & 0o077:
        raise SyncError("unsafe_extraction_directory")
    try:
        with mac_zip(path) as package:
            entries, _ = mac_zip_members(package)
            for info, kind in entries:
                target = child(destination, info.filename.rstrip("/"))
                if kind == "directory":
                    target.mkdir(mode=0o700, parents=True, exist_ok=True)
                elif kind == "file":
                    target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
                    fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
                    with os.fdopen(fd, "wb") as output, package.open(info) as stream:
                        remaining = info.file_size
                        while chunk := stream.read(min(CHUNK, remaining + 1)):
                            if len(chunk) > remaining:
                                raise SyncError("zip_expansion_limit")
                            output.write(chunk)
                            remaining -= len(chunk)
                        if remaining:
                            raise SyncError("incomplete_zip_entry")
                    target.chmod(((info.external_attr >> 16) & 0o755) | 0o600)
            for info, kind in entries:
                if kind == "link":
                    target = child(destination, info.filename)
                    target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
                    os.symlink(package.read(info).decode("utf-8"), target)
        return destination / "Claude.app"
    except (zipfile.BadZipFile, RuntimeError, EOFError, NotImplementedError) as exc:
        raise SyncError("invalid_zip") from exc


def inspect_package(path: Path, source: dict) -> dict:
    regular(path)
    if source["format"] == "zip":
        try:
            with mac_zip(path) as package:
                _, info = mac_zip_members(package)
            version = info.get("CFBundleShortVersionString")
            version_tuple(version)
            if info.get("CFBundleIdentifier") != source["identity"]:
                raise SyncError("wrong_package_identity")
            return {"version": version, "identity": source["identity"], "signature": "pending_native_verification", "compatibility": "not_tested"}
        except (zipfile.BadZipFile, RuntimeError, EOFError, NotImplementedError) as exc:
            raise SyncError("invalid_zip") from exc
    if source["format"] == "dmg":
        with path.open("rb") as stream:
            if path.stat().st_size < 512:
                raise SyncError("invalid_dmg")
            stream.seek(-512, os.SEEK_END)
            if stream.read(4) != b"koly":
                raise SyncError("invalid_dmg")
        return {"version": "unknown", "identity": "pending_native_inspection", "signature": "pending_native_verification", "compatibility": "not_tested"}
    try:
        with zipfile.ZipFile(path) as package:
            infos = package.infolist()
            names = [info.filename for info in infos]
            if len(names) != len(set(name.casefold() for name in names)):
                raise SyncError("ambiguous_archive")
            for info in infos:
                parts = PurePosixPath(info.filename).parts
                if info.filename.startswith(("/", "\\")) or "\\" in info.filename or ".." in parts or stat.S_ISLNK(info.external_attr >> 16):
                    raise SyncError("unsafe_archive")
            for name in ("AppxManifest.xml", "AppxSignature.p7x", "AppxBlockMap.xml"):
                if name not in names:
                    raise SyncError("incomplete_msix")
            info = package.getinfo("AppxManifest.xml")
            if info.file_size > 1024 * 1024:
                raise SyncError("manifest_too_large")
            xml = package.read(info)
            if b"<!DOCTYPE" in xml.upper() or b"<!ENTITY" in xml.upper():
                raise SyncError("unsafe_xml")
            manifest = ET.fromstring(xml)
            identity = manifest.find("{*}Identity")
            if identity is None or identity.attrib.get("Name") != source["identity"]:
                raise SyncError("wrong_package_identity")
            if identity.attrib.get("ProcessorArchitecture") != source["architecture"]:
                raise SyncError("wrong_package_architecture")
            version = identity.attrib.get("Version", "")
            publisher = identity.attrib.get("Publisher", "")
            if not re.fullmatch(r"\d+(?:\.\d+){3}", version) or not publisher:
                raise SyncError("invalid_package_identity")
            # Presence of AppxSignature is NOT evidence of signature trust.
            return {"version": version, "identity": source["identity"], "publisher": publisher,
                    "signature": "pending_native_verification", "compatibility": "not_tested"}
    except (zipfile.BadZipFile, ET.ParseError, RuntimeError, UnicodeError) as exc:
        raise SyncError("invalid_msix") from exc


def used_bytes(root: Path) -> int:
    total = 0
    for directory, directories, files in os.walk(root, followlinks=False):
        for name in directories + files:
            path = Path(directory) / name
            if path.is_symlink():
                raise SyncError("unsafe_symlink")
            if name in files:
                regular(path)
                total += path.stat().st_size
    return total


def candidate_path(root: Path, entry: dict) -> Path:
    relative = entry.get("path", "")
    if not re.fullmatch(r"artifacts/[a-f0-9]{64}\.(msix|dmg|zip)", relative):
        raise SyncError("invalid_candidate_path")
    return child(root, relative)


def valid_candidate(root: Path, entry: dict, metadata: dict) -> bool:
    if not entry or entry.get("metadata", {}).get("fingerprint") != metadata["fingerprint"]:
        return False
    path = candidate_path(root, entry)
    regular(path)
    if not path.exists():
        return False
    if path.stat().st_size != metadata["size"] or digest(path) != entry.get("sha256"):
        raise SyncError("cached_artifact_corrupt")
    return True


def reconciled_candidate(root: Path, metadata: dict) -> dict | None:
    directory = child(root, "artifacts")
    if not directory.exists():
        return None
    for path in sorted(directory.glob("*.json")):
        regular(path)
        entry = read_json(path, {})
        if valid_candidate(root, entry, metadata):
            return entry
    return None


def download(root: Path, source: dict, metadata: dict, transport: Transport, budget: int) -> dict:
    fingerprint = metadata["fingerprint"]
    partial = child(root, f"partials/{fingerprint}.part")
    receipt = child(root, f"partials/{fingerprint}.json")
    partial.parent.mkdir(mode=0o700, exist_ok=True)
    regular(partial)
    old = read_json(receipt, {})
    if old and old != metadata:
        raise SyncError("stale_partial_metadata")
    if partial.exists() and not old:
        raise SyncError("orphan_partial")
    offset = partial.stat().st_size if partial.exists() else 0
    if offset > metadata["size"]:
        raise SyncError("oversized_partial")
    remaining = metadata["size"] - offset
    # The scan prunes everything except the current and immediately previous
    # candidate before reaching this bound.
    if used_bytes(root) + remaining + CHUNK > budget or shutil.disk_usage(root).free < remaining + CHUNK:
        raise SyncError("cache_quota_or_disk_full")
    atomic_json(receipt, metadata)
    if remaining:
        request_headers = {"If-Match": metadata["etag"]}
        if offset:
            request_headers.update({"Range": f"bytes={offset}-", "If-Range": metadata["etag"]})
        with transport.request(source, "GET", metadata["url"], request_headers) as response:
            headers = headers_of(response)
            if response.status == 412:
                raise SyncError("source_changed")
            if offset and response.status == 200 and headers.get("etag") == metadata["etag"] and headers.get("content-length") == str(metadata["size"]):
                # Some official CDNs ignore Range. A complete same-validator 200
                # can safely restart our own partial after all headers validate.
                offset = 0
                remaining = metadata["size"]
            if response.status != (206 if offset else 200):
                raise SyncError("resume_not_supported" if offset and response.status == 200 else f"download_http_{response.status}")
            check_type(headers)
            if headers.get("etag") != metadata["etag"]:
                raise SyncError("source_changed")
            if positive_length(headers.get("content-length", "")) != remaining:
                raise SyncError("download_length_mismatch")
            if offset and headers.get("content-range") != f"bytes {offset}-{metadata['size'] - 1}/{metadata['size']}":
                raise SyncError("invalid_content_range")
            if not offset and headers.get("content-range"):
                raise SyncError("unexpected_content_range")
            flags = os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW | (os.O_APPEND if offset else os.O_TRUNC)
            fd = os.open(partial, flags, 0o600)
            deadline = time.monotonic() + 1800
            try:
                with os.fdopen(fd, "wb") as stream:
                    while remaining:
                        if time.monotonic() > deadline:
                            raise SyncError("download_deadline")
                        chunk = response.read(min(CHUNK, remaining))
                        if not chunk:
                            raise SyncError("incomplete_download")
                        if len(chunk) > remaining:
                            raise SyncError("oversized_download")
                        stream.write(chunk)
                        remaining -= len(chunk)
                    stream.flush()
                    os.fsync(stream.fileno())
            except (OSError, http.client.HTTPException) as exc:
                raise SyncError("download_interrupted") from exc
    if partial.stat().st_size != metadata["size"]:
        raise SyncError("download_length_mismatch")
    package = inspect_package(partial, source)
    if metadata["versionHint"] != "unknown" and package["version"] != metadata["versionHint"]:
        raise SyncError("version_metadata_mismatch")
    sha256 = digest(partial)
    relative = f"artifacts/{sha256}.{source['format']}"
    destination = child(root, relative)
    destination.parent.mkdir(mode=0o700, exist_ok=True)
    regular(destination)
    if destination.exists():
        if digest(destination) != sha256:
            raise SyncError("artifact_collision")
        partial.unlink()  # Exact duplicate produced by this source, not a previous version.
    else:
        os.replace(partial, destination)
    result = {"path": relative, "sha256": sha256, "metadata": metadata, "package": package, "stagedAt": utc()}
    # Immutable per-object receipt permits reconciliation if catalog save is interrupted.
    atomic_json(child(root, f"artifacts/{sha256}.json"), result)
    return result


def sources(path: Path = SOURCE_FILE) -> list[dict]:
    config = read_json(path, {})
    if config.get("schemaVersion") != 1 or not isinstance(config.get("sources"), list):
        raise SyncError("invalid_sources_config")
    values = config["sources"]
    ids = [source.get("id", "") for source in values]
    expected = {"codex-macos-arm64", "codex-windows-x64", "codex-windows-arm64", "claude-macos-universal", "claude-windows-x64", "claude-windows-arm64"}
    if len(ids) != 6 or set(ids) != expected:
        raise SyncError("invalid_source_inventory")
    for source in values:
        expected_format = "zip" if source["id"] == "claude-macos-universal" else "dmg" if source["platform"] == "macos" else "msix"
        if source["id"] != f"{source['app']}-{source['platform']}-{source['architecture']}" or source["format"] != expected_format:
            raise SyncError("invalid_source_identity")
        validate_url(source, source["url"])
    return values


def status(catalog: dict, configured: list[dict]) -> dict:
    result = []
    for source in configured:
        row = catalog.get("sources", {}).get(source["id"], {})
        result.append({"id": source["id"], "status": row.get("status", "never_checked"),
                       "version": row.get("candidate", {}).get("package", {}).get("version", row.get("metadata", {}).get("versionHint", "unknown")),
                       "error": row.get("error", "")})
    return {"schemaVersion": 1, "service": "yeschoy-vendor-sync", "checkedAt": catalog.get("checkedAt", "never"), "publicInstallersPublished": False, "sources": result}


def scan(root: Path, configured: list[dict], transport: Transport, fetch: bool, budget: int) -> tuple[dict, list[dict]]:
    catalog_path = child(root, "catalog.json")
    catalog = read_json(catalog_path, {"schemaVersion": 1, "sources": {}})
    if catalog.get("schemaVersion") != 1 or not isinstance(catalog.get("sources"), dict):
        raise SyncError("corrupt_catalog")
    retention = prune_private_cache(root, catalog)
    if retention["filesDeleted"]:
        print(json.dumps({"action": "pruned", **retention}), flush=True)
    events = []
    for source in configured:
        prior = catalog["sources"].get(source["id"], {})
        row = {**prior, "checkedAt": utc()}
        action = "unchanged"
        print(json.dumps({"source": source["id"], "action": "checking"}), flush=True)
        try:
            metadata = discover(source, transport)
            old_version = prior.get("candidate", {}).get("package", {}).get("version", "unknown")
            if old_version != "unknown" and metadata["versionHint"] != "unknown" and version_tuple(metadata["versionHint"]) < version_tuple(old_version):
                raise SyncError("version_regression")
            row.update({"metadata": metadata, "error": "", "status": "available"})
            if valid_candidate(root, prior.get("candidate", {}), metadata):
                row["status"] = "downloaded_unverified"
            elif fetch:
                candidate = reconciled_candidate(root, metadata)
                if candidate is None:
                    # Make room for the incoming generation without exceeding
                    # the two-generation policy. The current candidate remains;
                    # after a successful download it becomes the new previous.
                    if row.pop("previousCandidate", None):
                        catalog["sources"][source["id"]] = row
                        atomic_json(catalog_path, catalog)
                        prune_private_cache(root, catalog)
                    for attempt in range(3):
                        try:
                            candidate = download(root, source, metadata, transport, budget)
                            break
                        except SyncError as exc:
                            if str(exc) not in {"network_error", "download_interrupted", "incomplete_download"} or attempt == 2:
                                raise
                old_version = prior.get("candidate", {}).get("package", {}).get("version", "unknown")
                new_version = candidate["package"]["version"]
                if old_version != "unknown" and new_version != "unknown" and tuple(map(int, new_version.split("."))) < tuple(map(int, old_version.split("."))):
                    raise SyncError("version_regression")
                if prior.get("candidate"):
                    row["previousCandidate"] = prior["candidate"]
                row.update({"candidate": candidate, "status": "downloaded_unverified"})
                action = "staged"
            else:
                action = "discovered" if metadata != prior.get("metadata") else "unchanged"
        except SyncError as exc:
            row.update({"status": "failed", "error": str(exc)})
            action = "failed"
        except OSError:
            row.update({"status": "failed", "error": "local_io_error"})
            action = "failed"
        catalog["sources"][source["id"]] = row
        catalog["checkedAt"] = utc()
        atomic_json(catalog_path, catalog)
        event = {"source": source["id"], "action": action, "error": row["error"]}
        events.append(event)
        print(json.dumps(event), flush=True)
    retention = prune_private_cache(root, catalog)
    if retention["filesDeleted"]:
        print(json.dumps({"action": "pruned", **retention}), flush=True)
    atomic_json(child(root, "last-run.json"), {"checkedAt": catalog["checkedAt"], "events": events})
    return status(catalog, configured), events


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("scan", "status", "prune"))
    parser.add_argument("--state-dir", type=Path, required=True)
    parser.add_argument("--download", action="store_true")
    parser.add_argument("--budget-bytes", type=int, default=0)
    args = parser.parse_args()
    os.umask(0o077)
    try:
        configured = sources()
        root = private_root(args.state_dir, create=args.command != "status")
        if args.command == "status":
            # Atomic catalog replacement makes reads safe during an active scan.
            result = status(read_json(child(root, "catalog.json"), {}), configured)
        elif args.command == "prune":
            with locked(root):
                result = prune_private_cache(
                    root,
                    read_json(child(root, "catalog.json"), {"schemaVersion": 1, "sources": {}}),
                )
        else:
            with locked(root):
                if args.download and args.budget_bytes <= CHUNK:
                    raise SyncError("explicit_cache_budget_required")
                result, _ = scan(root, configured, Transport(), args.download, args.budget_bytes)
        print(json.dumps(result, ensure_ascii=False, indent=2))
        return 2 if args.command == "scan" and any(row["status"] == "failed" for row in result["sources"]) else 0
    except SyncError as exc:
        print(json.dumps({"service": "yeschoy-vendor-sync", "error": str(exc)}), file=sys.stderr)
        return 3
    except OSError:
        print(json.dumps({"service": "yeschoy-vendor-sync", "error": "local_io_error"}), file=sys.stderr)
        return 3


if __name__ == "__main__":
    raise SystemExit(main())
