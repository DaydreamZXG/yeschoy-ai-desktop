#!/usr/bin/env python3
"""Publish or disable one signed Yeschoy Tauri update channel atomically."""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import sys
from urllib.parse import quote

ORIGIN = "https://ergou.qzz.io"
TARGETS = ("darwin-aarch64", "darwin-x86_64", "windows-x86_64")
INSTALLER_TARGETS = ("macos-universal", "windows-x86_64")
VERSION = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
FILE_NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,159}$")


class PublishError(RuntimeError):
    pass


def semantic_version(value: str) -> tuple[int, int, int]:
    match = VERSION.fullmatch(value)
    if not match:
        raise PublishError("version_must_be_stable_semver")
    return tuple(int(part) for part in match.groups())  # type: ignore[return-value]


def guarded_root(value: str) -> Path:
    root = Path(value)
    if not root.is_absolute() or root.is_symlink():
        raise PublishError("root_must_be_absolute_real_directory")
    root = root.resolve(strict=True)
    if not root.is_dir() or len(root.parts) < 4 or root in (Path("/"), Path.home()):
        raise PublishError("unsafe_public_root")
    return root


def regular_file(value: str, suffix: str | None = None) -> Path:
    path = Path(value)
    if path.is_symlink() or not path.is_file():
        raise PublishError("asset_must_be_regular_file")
    mode = path.stat().st_mode
    if not stat.S_ISREG(mode):
        raise PublishError("asset_must_be_regular_file")
    if suffix and not path.name.endswith(suffix):
        raise PublishError("unexpected_file_suffix")
    if not FILE_NAME.fullmatch(path.name):
        raise PublishError("unsafe_asset_name")
    if path.stat().st_size <= 0:
        raise PublishError("empty_asset")
    return path.resolve(strict=True)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def parse_platform(value: str) -> tuple[str, Path, Path]:
    try:
        target, paths = value.split("=", 1)
        artifact_value, signature_value = paths.split(",", 1)
    except ValueError as error:
        raise PublishError("platform_format_is_target_artifact_signature") from error
    if target not in TARGETS:
        raise PublishError("unsupported_update_target")
    artifact = regular_file(artifact_value)
    signature = regular_file(signature_value, ".sig")
    signature_text = signature.read_text(encoding="utf-8").strip()
    if not signature_text or len(signature_text) > 16_384:
        raise PublishError("invalid_signature_text")
    if any(ord(character) < 32 and character not in "\r\n\t" for character in signature_text):
        raise PublishError("invalid_signature_text")
    return target, artifact, signature


def parse_installer(value: str) -> tuple[str, Path]:
    try:
        target, artifact_value = value.split("=", 1)
    except ValueError as error:
        raise PublishError("installer_format_is_target_artifact") from error
    if target not in INSTALLER_TARGETS:
        raise PublishError("unsupported_installer_target")
    expected = ".dmg" if target == "macos-universal" else "-installer.exe"
    return target, regular_file(artifact_value, expected)


def ensure_directory(path: Path) -> None:
    current = path
    missing: list[Path] = []
    while not current.exists():
        missing.append(current)
        current = current.parent
    if current.is_symlink() or not current.is_dir():
        raise PublishError("publication_parent_is_not_real_directory")
    for directory in reversed(missing):
        directory.mkdir(mode=0o755)


def immutable_copy(source: Path, destination: Path) -> dict[str, object]:
    ensure_directory(destination.parent)
    source_hash = sha256(source)
    if destination.exists():
        if destination.is_symlink() or not destination.is_file():
            raise PublishError("immutable_destination_conflict")
        if destination.stat().st_size != source.stat().st_size or sha256(destination) != source_hash:
            raise PublishError("immutable_destination_conflict")
    else:
        descriptor = os.open(
            destination,
            os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0),
            0o644,
        )
        try:
            with os.fdopen(descriptor, "wb") as output, source.open("rb") as input_stream:
                shutil.copyfileobj(input_stream, output, 1024 * 1024)
                output.flush()
                os.fsync(output.fileno())
        except BaseException:
            destination.unlink(missing_ok=True)
            raise
        if sha256(destination) != source_hash:
            destination.unlink(missing_ok=True)
            raise PublishError("immutable_copy_hash_mismatch")
    return {
        "sha256": source_hash,
        "size": source.stat().st_size,
        "fileName": destination.name,
    }


def atomic_write(path: Path, payload: bytes, mode: int = 0o644) -> None:
    ensure_directory(path.parent)
    if path.is_symlink():
        raise PublishError("atomic_target_is_symlink")
    temporary = path.with_name(f".{path.name}.tmp-{os.getpid()}")
    descriptor = os.open(
        temporary,
        os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0),
        mode,
    )
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        temporary.unlink(missing_ok=True)


def read_json(path: Path) -> dict[str, object]:
    if path.is_symlink() or not path.is_file():
        raise PublishError("required_json_is_not_regular_file")
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise PublishError("required_json_is_not_object")
    return value


def health_bytes(health: dict[str, object], published: bool) -> bytes:
    required = {
        "schemaVersion": 1,
        "service": "yeschoy-download-origin",
        "status": "origin_ready",
    }
    if any(health.get(key) != value for key, value in required.items()):
        raise PublishError("unexpected_health_identity")
    if not isinstance(health.get("thirdPartyInstallersPublished"), bool):
        raise PublishError("invalid_third_party_health_state")
    updated = dict(health)
    updated["updatesPublished"] = published
    return (json.dumps(updated, ensure_ascii=False, indent=2) + "\n").encode()


def rollback_bytes(path: Path, previous: bytes | None) -> None:
    if previous is None:
        if path.exists():
            if path.is_symlink() or not path.is_file():
                raise PublishError("rollback_target_conflict")
            path.unlink()
        return
    atomic_write(path, previous)


def archive_manifest(path: Path, payload: bytes | None) -> str | None:
    if payload is None:
        return None
    digest = hashlib.sha256(payload).hexdigest()
    destination = path.parent / "history" / f"stable-{digest}.json"
    if destination.exists():
        if destination.is_symlink() or destination.read_bytes() != payload:
            raise PublishError("manifest_history_conflict")
    else:
        atomic_write(destination, payload)
    return digest


def previous_manifest(path: Path) -> bytes | None:
    payload = path.read_bytes() if path.is_file() and not path.is_symlink() else None
    if path.exists() and payload is None:
        raise PublishError("stable_manifest_conflict")
    return payload


def require_newer_version(payload: bytes | None, version: tuple[int, int, int]) -> None:
    if payload is None:
        return
    current = json.loads(payload)
    current_version = current.get("version") if isinstance(current, dict) else None
    if not isinstance(current_version, str) or version <= semantic_version(current_version):
        raise PublishError("version_must_increase")


def publish(args: argparse.Namespace) -> dict[str, object]:
    root = guarded_root(args.root)
    version_tuple = semantic_version(args.version)
    if len(args.platform) != len(TARGETS):
        raise PublishError("all_supported_targets_are_required")
    parsed = [parse_platform(value) for value in args.platform]
    if {target for target, _, _ in parsed} != set(TARGETS):
        raise PublishError("each_supported_target_is_required_once")
    notes_path = regular_file(args.notes)
    notes = notes_path.read_text(encoding="utf-8").strip()
    if not notes or len(notes) > 600:
        raise PublishError("release_notes_must_be_1_to_600_characters")

    installer_values = getattr(args, "installer", None)
    installers: list[tuple[str, Path]] = []
    if installer_values is not None:
        if len(installer_values) != len(INSTALLER_TARGETS):
            raise PublishError("all_supported_installers_are_required")
        installers = [parse_installer(value) for value in installer_values]
        if {target for target, _ in installers} != set(INSTALLER_TARGETS):
            raise PublishError("each_supported_installer_is_required_once")

    updates = root / "updates"
    stable = updates / "stable.json"
    download_manifest_path = root / "releases" / "yeschoy.json"
    health_path = root / "health.json"
    health = read_json(health_path)
    previous_update = previous_manifest(stable)
    previous_download = previous_manifest(download_manifest_path) if installers else None
    # Emergency update-channel disablement intentionally keeps the website
    # installer manifest online. Both mutable heads therefore enforce their
    # own monotonic version even when either counterpart is absent.
    require_newer_version(previous_update, version_tuple)
    require_newer_version(previous_download, version_tuple)

    release_root = updates / "releases" / args.version
    platforms: dict[str, object] = {}
    receipts: dict[str, object] = {}
    by_source: dict[Path, tuple[Path, dict[str, object]]] = {}
    for target, artifact, signature in parsed:
        destination = release_root / artifact.name
        if artifact not in by_source:
            receipt = immutable_copy(artifact, destination)
            by_source[artifact] = (destination, receipt)
        destination, receipt = by_source[artifact]
        signature_text = signature.read_text(encoding="utf-8").strip()
        platforms[target] = {
            "url": f"{ORIGIN}/{quote(destination.relative_to(root).as_posix(), safe='/._-')}",
            "signature": signature_text,
        }
        receipts[target] = receipt

    manifest = {
        "version": args.version,
        "notes": notes,
        "pub_date": dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z"),
        "platforms": platforms,
    }
    manifest_bytes = (json.dumps(manifest, ensure_ascii=False, indent=2) + "\n").encode()
    download_manifest_bytes: bytes | None = None
    installer_receipts: dict[str, object] = {}
    if installers:
        download_platforms: dict[str, object] = {}
        for target, artifact in installers:
            destination = root / "releases" / "yeschoy" / args.version / artifact.name
            receipt = immutable_copy(artifact, destination)
            installer_receipts[target] = receipt
            download_platforms[target] = {
                "url": f"{ORIGIN}/{quote(destination.relative_to(root).as_posix(), safe='/._-')}",
                "sha256": receipt["sha256"],
                "size": receipt["size"],
            }
        download_manifest_bytes = (
            json.dumps(
                {
                    "schemaVersion": 1,
                    "version": args.version,
                    "platforms": download_platforms,
                },
                ensure_ascii=False,
                indent=2,
            )
            + "\n"
        ).encode()
    previous_update_hash = archive_manifest(stable, previous_update)
    previous_download_hash = archive_manifest(download_manifest_path, previous_download)
    previous_health = health_path.read_bytes()
    try:
        if download_manifest_bytes is not None:
            atomic_write(download_manifest_path, download_manifest_bytes)
        atomic_write(stable, manifest_bytes)
        atomic_write(health_path, health_bytes(health, True))
    except BaseException:
        rollback_bytes(stable, previous_update)
        if installers:
            rollback_bytes(download_manifest_path, previous_download)
        atomic_write(health_path, previous_health)
        raise
    return {
        "status": "published",
        "version": args.version,
        "manifestSha256": hashlib.sha256(manifest_bytes).hexdigest(),
        "previousManifestSha256": previous_update_hash,
        "downloadManifestSha256": hashlib.sha256(download_manifest_bytes).hexdigest()
        if download_manifest_bytes is not None
        else None,
        "previousDownloadManifestSha256": previous_download_hash,
        "platforms": receipts,
        "installers": installer_receipts,
    }


def manifest_from_history(path: Path, digest: str) -> bytes:
    if not SHA256.fullmatch(digest):
        raise PublishError("invalid_restore_manifest_sha256")
    source = path.parent / "history" / f"stable-{digest}.json"
    payload = read_json(source)
    encoded = (json.dumps(payload, ensure_ascii=False, indent=2) + "\n").encode()
    # Preserve the exact archived bytes. Re-encoding is only a bounded JSON
    # validity check; the digest applies to the original publication bytes.
    del encoded
    original = source.read_bytes()
    if hashlib.sha256(original).hexdigest() != digest:
        raise PublishError("restore_manifest_hash_mismatch")
    return original


def rollback(args: argparse.Namespace) -> dict[str, object]:
    root = guarded_root(args.root)
    stable = root / "updates" / "stable.json"
    download = root / "releases" / "yeschoy.json"
    health_path = root / "health.json"
    health = read_json(health_path)
    current = previous_manifest(stable)
    current_download = previous_manifest(download)
    if current is None or hashlib.sha256(current).hexdigest() != args.expected_sha256:
        raise PublishError("published_manifest_changed")
    if args.expected_download_sha256 == "none":
        if current_download is not None:
            raise PublishError("published_download_manifest_changed")
    elif (
        current_download is None
        or hashlib.sha256(current_download).hexdigest() != args.expected_download_sha256
    ):
        raise PublishError("published_download_manifest_changed")
    restore = (
        None
        if args.restore_sha256 == "none"
        else manifest_from_history(stable, args.restore_sha256)
    )
    restore_download = (
        None
        if args.restore_download_sha256 == "none"
        else manifest_from_history(download, args.restore_download_sha256)
    )
    archive_manifest(stable, current)
    archive_manifest(download, current_download)
    previous_health = health_path.read_bytes()
    try:
        rollback_bytes(download, restore_download)
        rollback_bytes(stable, restore)
        atomic_write(health_path, health_bytes(health, restore is not None))
    except BaseException:
        rollback_bytes(download, current_download)
        rollback_bytes(stable, current)
        atomic_write(health_path, previous_health)
        raise
    return {
        "status": "rolled_back",
        "restoredManifestSha256": args.restore_sha256,
        "restoredDownloadManifestSha256": args.restore_download_sha256,
    }


def disable(args: argparse.Namespace) -> dict[str, object]:
    root = guarded_root(args.root)
    stable = root / "updates" / "stable.json"
    health_path = root / "health.json"
    health = read_json(health_path)
    previous_health = health_path.read_bytes()
    if not stable.exists():
        atomic_write(health_path, health_bytes(health, False))
        return {"status": "already_disabled"}
    if stable.is_symlink() or not stable.is_file():
        raise PublishError("stable_manifest_conflict")
    payload = stable.read_bytes()
    digest = hashlib.sha256(payload).hexdigest()
    disabled = root / "updates" / "disabled" / f"stable-{digest}.json"
    ensure_directory(disabled.parent)
    if disabled.exists():
        if disabled.is_symlink() or disabled.read_bytes() != payload:
            raise PublishError("disabled_manifest_conflict")
        stable.unlink()
    else:
        os.replace(stable, disabled)
    try:
        atomic_write(health_path, health_bytes(health, False))
    except BaseException:
        atomic_write(stable, payload)
        atomic_write(health_path, previous_health)
        raise
    return {"status": "disabled", "manifestSha256": digest}


def status(args: argparse.Namespace) -> dict[str, object]:
    root = guarded_root(args.root)
    stable = root / "updates" / "stable.json"
    health = read_json(root / "health.json")
    return {
        "status": "published" if stable.is_file() and not stable.is_symlink() else "disabled",
        "updatesPublished": health.get("updatesPublished"),
        "manifestSha256": sha256(stable) if stable.is_file() and not stable.is_symlink() else None,
    }


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    commands = result.add_subparsers(dest="command", required=True)
    publish_parser = commands.add_parser("publish")
    publish_parser.add_argument("--root", required=True)
    publish_parser.add_argument("--version", required=True)
    publish_parser.add_argument("--notes", required=True)
    publish_parser.add_argument("--platform", action="append", required=True)
    publish_parser.add_argument("--installer", action="append", required=True)
    publish_parser.set_defaults(handler=publish)
    rollback_parser = commands.add_parser("rollback")
    rollback_parser.add_argument("--root", required=True)
    rollback_parser.add_argument("--expected-sha256", required=True)
    rollback_parser.add_argument("--restore-sha256", required=True)
    rollback_parser.add_argument("--expected-download-sha256", required=True)
    rollback_parser.add_argument("--restore-download-sha256", required=True)
    rollback_parser.set_defaults(handler=rollback)
    disable_parser = commands.add_parser("disable")
    disable_parser.add_argument("--root", required=True)
    disable_parser.set_defaults(handler=disable)
    status_parser = commands.add_parser("status")
    status_parser.add_argument("--root", required=True)
    status_parser.set_defaults(handler=status)
    return result


def main() -> int:
    try:
        args = parser().parse_args()
        print(json.dumps(args.handler(args), ensure_ascii=False, indent=2))
        return 0
    except (PublishError, OSError, UnicodeError, json.JSONDecodeError) as error:
        print(json.dumps({"status": "failed", "reasonCode": str(error)}, ensure_ascii=False))
        return 1


if __name__ == "__main__":
    sys.exit(main())
