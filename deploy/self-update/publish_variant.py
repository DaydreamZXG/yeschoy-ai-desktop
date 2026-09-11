#!/usr/bin/env python3
"""Local-build release promotion. One pinned variant, real signature verification, shared lock."""
from __future__ import annotations

import argparse
import base64
import contextlib
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

from publish import (PublishError, atomic_write, guarded_root, health_bytes,
                     immutable_copy, previous_manifest, read_json, regular_file,
                     require_newer_version, rollback_bytes, semantic_version, sha256)

ORIGIN = "https://ergou.qzz.io"
VARIANTS = ("official", "partner")


def encoded(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode()


def channel_config(path: Path, variant: str) -> dict:
    registry = read_json(path)
    if registry.get("schemaVersion") != 1 or variant not in VARIANTS:
        raise PublishError("invalid_channel_registry")
    if registry["official"]["publicKey"] == registry["partner"]["publicKey"]:
        raise PublishError("channel_keys_must_differ")
    value = registry[variant]
    expected_origin = "https://yeschoy.com" if variant == "official" else "https://ai.yeschoy.io"
    if value["authorizationOrigin"] != expected_origin or value["endpoint"] != f"{ORIGIN}/updates/{variant}/stable.json":
        raise PublishError("channel_identity_mismatch")
    return value


def verify_signature(artifact: Path, signature: Path, public_key: str, minisign: str) -> str:
    if signature.stat().st_size > 16384:
        raise PublishError("invalid_signature_size")
    text = signature.read_text().strip()
    try:
        public = base64.b64decode(public_key, validate=True)
        detached = base64.b64decode(text, validate=True)
    except ValueError as error:
        raise PublishError("invalid_signature_encoding") from error
    with tempfile.TemporaryDirectory(prefix="yeschoy-verify-") as folder:
        key = Path(folder) / "updater.pub"
        sig = Path(folder) / "artifact.minisig"
        key.write_bytes(public)
        sig.write_bytes(detached)
        result = subprocess.run([minisign, "-Vm", str(artifact), "-p", str(key), "-x", str(sig)],
                                capture_output=True, timeout=60, check=False)
        if result.returncode != 0:
            raise PublishError("signature_verification_failed")
    return text


def heads(root: Path, variant: str) -> tuple[Path, Path]:
    return root / "updates" / variant / "stable.json", root / "releases" / f"yeschoy-{variant}.json"


def safe_descendant(path: Path, anchor: Path) -> None:
    path.relative_to(anchor)
    for entry in (path, *path.parents):
        if entry.is_symlink():
            raise PublishError("publication_path_is_symlink")
        if entry == anchor:
            return
    raise PublishError("publication_path_escaped")


def refreshed_health(root: Path) -> bytes:
    health = read_json(root / "health.json")
    channels = {v: heads(root, v)[0].is_file() for v in VARIANTS}
    health["updateChannels"] = channels
    return health_bytes(health, any(channels.values()))


@contextlib.contextmanager
def publication_lock(path: Path):
    # This path is private operational state, never an artifact in public/.
    descriptor = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(descriptor, fcntl.LOCK_EX)
        yield
    finally:
        os.close(descriptor)


def promote(args, root: Path) -> dict:
    version = semantic_version(args.version)
    variant = args.variant
    config = channel_config(Path(args.channel_config), variant)
    update_head, download_head = heads(root, variant)
    for path in (*heads(root, "official"), *heads(root, "partner"), root / "health.json"):
        safe_descendant(path, root)
    old_update, old_download = previous_manifest(update_head), previous_manifest(download_head)
    old_health = (root / "health.json").read_bytes()
    # Validate health before copying or changing any published head.
    health_bytes(read_json(root / "health.json"), False)
    for previous in (old_update, old_download):
        require_newer_version(previous, version)
        if previous and json.loads(previous).get("variant") != variant:
            raise PublishError("previous_variant_mismatch")
    state_root = root.parent / "self-update-state" / variant
    watermark = state_root / "highest-version.json"
    safe_descendant(watermark, root.parent)
    if watermark.exists():
        require_newer_version(watermark.read_bytes(), version)
    notes = Path(args.notes).read_text().strip()
    if not 1 <= len(notes) <= 600:
        raise PublishError("release_notes_must_be_1_to_600_characters")
    prefix = f"yeschoy-{args.version}-{variant}"
    names = {
        "windows": f"{prefix}-windows-x86_64-installer.exe",
        "macos": f"{prefix}-macos-universal.app.tar.gz",
        "dmg": f"{prefix}-macos-universal-installer.dmg",
    }
    staged: dict[str, tuple[Path, str]] = {}
    # Verify snapshots, then copy those exact verified bytes; source edits
    # cannot race signature verification and public promotion.
    with tempfile.TemporaryDirectory(prefix="yeschoy-candidate-") as temporary:
        for kind, name in names.items():
            artifact = regular_file(str(Path(args.candidate) / name))
            signature = regular_file(str(Path(args.candidate) / f"{name}.sig"), ".sig")
            snapshot = Path(temporary) / name
            immutable_copy(artifact, snapshot)
            staged[kind] = (snapshot, verify_signature(snapshot, signature, config["publicKey"], args.minisign))
        entries = {}
        for kind, (snapshot, signature) in staged.items():
            destination = root / "updates" / "releases" / variant / args.version / snapshot.name
            safe_descendant(destination, root)
            receipt = immutable_copy(snapshot, destination)
            entries[kind] = {"url": f"{ORIGIN}/{destination.relative_to(root).as_posix()}",
                             "signature": signature, "sha256": receipt["sha256"], "size": receipt["size"]}
    manifest = {"schemaVersion": 2, "variant": variant, "version": args.version, "notes": notes,
                "platforms": {"windows-x86_64": entries["windows"], "darwin-x86_64": entries["macos"], "darwin-aarch64": entries["macos"]}}
    download = {"schemaVersion": 2, "variant": variant, "version": args.version,
                "platforms": {"windows-x86_64": entries["windows"], "macos-universal": entries["dmg"]}}
    update_bytes, download_bytes = encoded(manifest), encoded(download)
    digest = hashlib.sha256(update_bytes).hexdigest()
    download_digest = hashlib.sha256(download_bytes).hexdigest()
    recovery = {"variant": variant, "expectedHead": digest, "expectedDownload": download_digest,
                "update": base64.b64encode(old_update).decode() if old_update else None,
                "download": base64.b64encode(old_download).decode() if old_download else None}
    atomic_write(state_root / f"rollback-{digest}.json", encoded(recovery), 0o600)
    # Even after withdrawal, previously published versions cannot be replayed.
    atomic_write(watermark, encoded({"version": args.version}), 0o600)
    try:
        atomic_write(download_head, download_bytes)
        atomic_write(update_head, update_bytes)
        atomic_write(root / "health.json", refreshed_health(root))
    except BaseException:
        rollback_bytes(update_head, old_update)
        rollback_bytes(download_head, old_download)
        atomic_write(root / "health.json", old_health)
        raise
    return {"status": "published", "variant": variant, "version": args.version,
            "manifestSha256": digest, "downloadManifestSha256": download_digest,
            "updateUrl": config["endpoint"], "downloadManifestUrl": f"{ORIGIN}/releases/yeschoy-{variant}.json"}


def rollback(args, root: Path) -> dict:
    if len(args.expected_sha256) != 64 or any(c not in "0123456789abcdef" for c in args.expected_sha256):
        raise PublishError("invalid_expected_hash")
    update, download = heads(root, args.variant)
    for path in (update, download, root / "health.json", root.parent / "self-update-state" / args.variant):
        safe_descendant(path, root.parent)
    recovery = read_json(root.parent / "self-update-state" / args.variant / f"rollback-{args.expected_sha256}.json")
    if recovery["variant"] != args.variant or recovery["expectedHead"] != args.expected_sha256:
        raise PublishError("recovery_identity_mismatch")
    if sha256(update) != recovery["expectedHead"] or sha256(download) != recovery["expectedDownload"]:
        raise PublishError("publication_changed")
    previous_update, previous_download = update.read_bytes(), download.read_bytes()
    previous_health = (root / "health.json").read_bytes()
    try:
        rollback_bytes(update, base64.b64decode(recovery["update"]) if recovery["update"] else None)
        rollback_bytes(download, base64.b64decode(recovery["download"]) if recovery["download"] else None)
        atomic_write(root / "health.json", refreshed_health(root))
    except BaseException:
        atomic_write(update, previous_update)
        atomic_write(download, previous_download)
        atomic_write(root / "health.json", previous_health)
        raise
    return {"status": "rolled_back", "variant": args.variant}


def disable(args, root: Path) -> dict:
    """Withdraw one update feed without withdrawing manual downloads."""
    update, _ = heads(root, args.variant)
    for path in (update, root / "health.json"):
        safe_descendant(path, root)
    if not args.expected_sha256 or sha256(update) != args.expected_sha256:
        raise PublishError("publication_changed")
    previous, health = update.read_bytes(), (root / "health.json").read_bytes()
    try:
        rollback_bytes(update, None)
        atomic_write(root / "health.json", refreshed_health(root))
    except BaseException:
        atomic_write(update, previous)
        atomic_write(root / "health.json", health)
        raise
    return {"status": "disabled", "variant": args.variant, "manualDownloadsPreserved": True}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("publish", "rollback", "disable"))
    parser.add_argument("--root", required=True)
    parser.add_argument("--variant", required=True, choices=VARIANTS)
    parser.add_argument("--lock", required=True)
    parser.add_argument("--candidate")
    parser.add_argument("--version")
    parser.add_argument("--notes")
    parser.add_argument("--channel-config")
    parser.add_argument("--minisign", default="minisign")
    parser.add_argument("--expected-sha256")
    args = parser.parse_args()
    try:
        root = guarded_root(args.root)
        lock = Path(args.lock)
        if not lock.is_absolute() or lock.resolve().is_relative_to(root):
            raise PublishError("lock_must_be_private_absolute_path")
        if args.action == "publish" and not all((args.candidate, args.version, args.notes, args.channel_config)):
            raise PublishError("missing_publish_inputs")
        if args.action in ("rollback", "disable") and not args.expected_sha256:
            raise PublishError("missing_expected_hash")
        with publication_lock(lock):
            action = {"publish": promote, "rollback": rollback, "disable": disable}[args.action]
            result = action(args, root)
        print(json.dumps(result, ensure_ascii=False, indent=2))
        return 0
    except (PublishError, OSError, ValueError, KeyError, subprocess.SubprocessError):
        # No candidate content or credentials in diagnostics.
        print(json.dumps({"status": "failed", "reasonCode": "release_verification_or_promotion_failed"}))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
