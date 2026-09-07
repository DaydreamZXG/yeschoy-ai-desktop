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
VERSION = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")
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

    updates = root / "updates"
    stable = updates / "stable.json"
    health_path = root / "health.json"
    health = read_json(health_path)
    previous_manifest = stable.read_bytes() if stable.is_file() and not stable.is_symlink() else None
    if stable.exists() and previous_manifest is None:
        raise PublishError("stable_manifest_conflict")
    if previous_manifest:
        current = json.loads(previous_manifest)
        current_version = current.get("version") if isinstance(current, dict) else None
        if not isinstance(current_version, str) or version_tuple <= semantic_version(current_version):
            raise PublishError("version_must_increase")

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
    previous_health = health_path.read_bytes()
    try:
        atomic_write(stable, manifest_bytes)
        atomic_write(health_path, health_bytes(health, True))
    except BaseException:
        rollback_bytes(stable, previous_manifest)
        atomic_write(health_path, previous_health)
        raise
    return {
        "status": "published",
        "version": args.version,
        "manifestSha256": hashlib.sha256(manifest_bytes).hexdigest(),
        "platforms": receipts,
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
    publish_parser.set_defaults(handler=publish)
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
