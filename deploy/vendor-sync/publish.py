#!/usr/bin/env python3
"""Operator-only publication of exact official-origin bytes.

The sync timer never runs this tool. Publication is an explicit operator action;
native receipts improve the public verification claim but are not required for
platforms where the client performs the final native trust check. This tool
does not grant vendor permission or replace end-device signature verification.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys
import tempfile

from sync import (CHUNK, MAX_BYTES, MAX_JSON_BYTES, SyncError, candidate_path, child,
                  digest, inspect_package, private_root, regular, sources,
                  strict_json, utc, validate_url, version_tuple)

ORIGIN = "https://ergou.qzz.io"
WINDOWS_PUBLISHERS = {
    "codex": "CN=50BDFD77-8903-4850-9FFE-6E8522F64D5B",
    "claude": 'CN="Anthropic, PBC", O="Anthropic, PBC", L=San Francisco, S=California, C=US, SERIALNUMBER=4860621, OID.2.5.4.15=Private Organization, OID.1.3.6.1.4.1.311.60.2.1.2=Delaware, OID.1.3.6.1.4.1.311.60.2.1.3=US',
}
CATALOG_KEYS = {"schemaVersion", "generatedAt", "artifacts"}
ARTIFACT_KEYS = {"sourceId", "app", "platform", "architecture", "format", "version", "url", "sha256", "size", "originUrl", "identity", "publisher", "verifiedAt", "verification"}
HEALTH = {"schemaVersion": 1, "service": "yeschoy-download-origin", "status": "origin_ready", "updatesPublished": False, "thirdPartyInstallersPublished": False}


def publisher(source: dict) -> str:
    return source["teamId"] if source["platform"] == "macos" else WINDOWS_PUBLISHERS[source["app"]]


def timestamp(value) -> datetime:
    try:
        if not isinstance(value, str) or not value or len(value) > 64:
            raise ValueError()
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
        if parsed.tzinfo is None or parsed > datetime.now(timezone.utc):
            raise ValueError()
        return parsed
    except ValueError as exc:
        raise SyncError("invalid_evidence_time") from exc


def exact(value, keys, error):
    if not isinstance(value, dict) or set(value) != keys:
        raise SyncError(error)


def no_links(path: Path) -> Path:
    path = path.absolute()
    for current in reversed((path, *path.parents)):
        if current.is_symlink():
            raise SyncError("unsafe_symlink")
    return path


def owned(path: Path, directory=False) -> None:
    metadata = path.lstat()
    expected = stat.S_ISDIR if directory else stat.S_ISREG
    if not expected(metadata.st_mode) or metadata.st_uid != os.geteuid() or metadata.st_mode & 0o022:
        raise SyncError("operator_owned_path_required")


def load(path: Path, operator=False) -> dict:
    no_links(path)
    regular(path)
    if operator:
        owned(path)
    if path.stat().st_size > MAX_JSON_BYTES:
        raise SyncError("json_too_large")
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(fd, "rb") as stream:
        return strict_json(stream.read(MAX_JSON_BYTES + 1))


def public_root(path: Path) -> Path:
    path = no_links(path)
    if path in {Path("/"), Path("/tmp"), Path("/var"), Path("/srv"), Path.home()}:
        raise SyncError("unsafe_public_root")
    owned(path, directory=True)
    health = load(child(path, "health.json"), operator=True)
    exact(health, set(HEALTH) | ({"updateChannels"} if "updateChannels" in health else set()), "invalid_origin_health")
    if type(health["schemaVersion"]) is not int or health["schemaVersion"] != 1 or health["service"] != HEALTH["service"] or health["status"] != "origin_ready" or type(health["updatesPublished"]) is not bool or type(health["thirdPartyInstallersPublished"]) is not bool:
        raise SyncError("invalid_origin_health")
    if "updateChannels" in health:
        channels = health["updateChannels"]
        if not isinstance(channels, dict) or set(channels) != {"official", "partner"} or any(type(v) is not bool for v in channels.values()) or health["updatesPublished"] != any(channels.values()):
            raise SyncError("invalid_origin_health")
    return path


def directory(path: Path) -> None:
    no_links(path)
    try:
        path.mkdir(mode=0o755)
    except FileExistsError:
        pass
    owned(path, directory=True)


@contextmanager
def locked(root: Path):
    import fcntl
    apps = child(root, "apps")
    directory(apps)
    path = child(apps, ".publish.lock")
    regular(path)
    fd = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        owned(path)
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            raise SyncError("publisher_already_running") from exc
        yield
    finally:
        os.close(fd)


def sync_directory(path: Path) -> None:
    fd = os.open(path, os.O_RDONLY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def atomic_public_json(path: Path, value: dict) -> None:
    regular(path)
    if path.exists():
        owned(path)
    encoded = (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()
    if len(encoded) > MAX_JSON_BYTES:
        raise SyncError("json_too_large")
    fd, name = tempfile.mkstemp(prefix=".publish-", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as output:
            output.write(encoded)
            output.flush()
            os.fchmod(output.fileno(), 0o644)
            os.fsync(output.fileno())
        os.replace(name, path)
        sync_directory(path.parent)
    finally:
        if os.path.lexists(name):
            os.unlink(name)  # Exact exclusive temporary file created above.


def artifact_path(root: Path, row: dict) -> Path:
    return child(root, f"apps/{row['sourceId']}/{row['sha256']}.{row['format']}")


def validate_artifact(row: dict, configured: dict, schema_version: int) -> None:
    exact(row, ARTIFACT_KEYS, "invalid_public_artifact")
    source = configured.get(row["sourceId"]) if isinstance(row["sourceId"], str) else None
    if not source:
        raise SyncError("unknown_public_source")
    for key in ("app", "platform", "architecture", "format", "identity"):
        if row[key] != source[key]:
            raise SyncError("public_identity_mismatch")
    allowed_verification = {"native_verified"} if schema_version == 1 else {"native_verified", "client_native_required"}
    if row["publisher"] != publisher(source) or row["verification"] not in allowed_verification:
        raise SyncError("public_identity_mismatch")
    if not isinstance(row["sha256"], str) or not re.fullmatch(r"[a-f0-9]{64}", row["sha256"]) or type(row["size"]) is not int or not 0 < row["size"] <= MAX_BYTES:
        raise SyncError("invalid_public_integrity")
    version_tuple(row["version"])
    timestamp(row["verifiedAt"])
    expected_url = f"{ORIGIN}/apps/{source['id']}/{row['sha256']}.{source['format']}"
    if row["url"] != expected_url:
        raise SyncError("invalid_public_url")
    parsed = validate_url(source, row["originUrl"])
    if not parsed.path.endswith("." + source["format"]):
        raise SyncError("invalid_package_origin")
    if source.get("resolver") == "release_feed" and not parsed.path.startswith(f"/releases/darwin/universal/{row['version']}/"):
        raise SyncError("release_version_mismatch")


def catalog(root: Path, configured: dict) -> dict:
    path = child(root, "apps/catalog.json")
    if not path.exists():
        return {"schemaVersion": 1, "generatedAt": utc(), "artifacts": []}
    value = load(path, operator=True)
    exact(value, CATALOG_KEYS, "invalid_public_catalog")
    schema_version = value["schemaVersion"]
    if type(schema_version) is not int or schema_version not in {1, 2} or not isinstance(value["artifacts"], list) or len(value["artifacts"]) > 6:
        raise SyncError("invalid_public_catalog")
    timestamp(value["generatedAt"])
    seen = set()
    for row in value["artifacts"]:
        validate_artifact(row, configured, schema_version)
        if row["sourceId"] in seen:
            raise SyncError("duplicate_public_source")
        seen.add(row["sourceId"])
        path = artifact_path(root, row)
        owned(path)
        if path.stat().st_size != row["size"] or digest(path) != row["sha256"]:
            raise SyncError("published_object_corrupt")
    return value


def prune_public_history_locked(root: Path, configured: dict, value: dict) -> dict:
    """Keep the catalog target and one immediately previous object per source."""
    current = {row["sourceId"]: row["sha256"] for row in value["artifacts"]}
    deleted = 0
    deleted_bytes = 0
    for source_id, source in configured.items():
        folder = child(root, f"apps/{source_id}")
        if not folder.exists():
            continue
        no_links(folder)
        owned(folder, directory=True)
        pattern = re.compile(rf"([a-f0-9]{{64}})\.{re.escape(source['format'])}")
        history = []
        for path in folder.iterdir():
            match = pattern.fullmatch(path.name)
            if match is None:
                continue
            owned(path)
            if match.group(1) != current.get(source_id):
                history.append(path)
        history.sort(key=lambda path: (path.stat().st_mtime_ns, path.name), reverse=True)
        for path in history[1:]:
            deleted_bytes += path.stat().st_size
            path.unlink()
            deleted += 1
        if len(history) > 1:
            sync_directory(folder)
    return {"status": "pruned", "filesDeleted": deleted, "bytesDeleted": deleted_bytes}


def prune_history(output_dir: Path) -> dict:
    configured = {source["id"]: source for source in sources()}
    root = public_root(output_dir)
    with locked(root):
        return prune_public_history_locked(root, configured, catalog(root, configured))


def verified_for(receipt: dict, source: dict, sha256: str, package: dict) -> dict:
    exact(receipt, {"schemaVersion", "source", "sha256", "checkedAt", "package", "verifier", "installation", "published"}, "invalid_native_receipt")
    native = receipt["package"]
    exact(native, {"version", "identity", "publisher", "signature", "compatibility"}, "invalid_native_receipt")
    expected = "macos_codesign_gatekeeper" if source["platform"] == "macos" else "windows_signtool_authenticode"
    if type(receipt["schemaVersion"]) is not int or receipt["schemaVersion"] != 1 or receipt["source"] != source["id"] or receipt["sha256"] != sha256 or receipt["verifier"] != expected or receipt["published"] is not False or receipt["installation"] != "not_tested" or native["signature"] != "native_verified" or native["compatibility"] != "not_tested":
        raise SyncError("native_verification_required")
    if native["identity"] != source["identity"] or native["publisher"] != publisher(source):
        raise SyncError("native_identity_mismatch")
    version_tuple(native["version"])
    if package["version"] != "unknown" and native["version"] != package["version"] or package.get("publisher", native["publisher"]) != native["publisher"]:
        raise SyncError("native_package_mismatch")
    timestamp(receipt["checkedAt"])
    return native


def copy_immutable(source: Path, destination: Path, sha256: str, size: int) -> None:
    regular(destination)
    if destination.exists():
        owned(destination)
        if destination.stat().st_size != size or digest(destination) != sha256:
            raise SyncError("immutable_object_conflict")
        return
    fd, name = tempfile.mkstemp(prefix=".object-", dir=destination.parent)
    try:
        check, total = hashlib.sha256(), 0
        input_fd = os.open(source, os.O_RDONLY | os.O_NOFOLLOW)
        with os.fdopen(fd, "wb") as output, os.fdopen(input_fd, "rb") as stream:
            while chunk := stream.read(CHUNK):
                total += len(chunk)
                if total > size:
                    raise SyncError("staged_object_changed")
                check.update(chunk)
                output.write(chunk)
            if total != size or check.hexdigest() != sha256:
                raise SyncError("staged_object_changed")
            output.flush()
            os.fchmod(output.fileno(), 0o644)
            os.fsync(output.fileno())
        # Hard-link creation is atomic and cannot replace an existing object.
        os.link(name, destination, follow_symlinks=False)
        sync_directory(destination.parent)
    finally:
        if os.path.lexists(name):
            os.unlink(name)


def reconcile_health(root: Path, value: dict) -> None:
    public_root(root)
    existing = load(child(root, "health.json"), operator=True)
    health = {**existing, "thirdPartyInstallersPublished": bool(value["artifacts"])}
    if existing != health:
        atomic_public_json(child(root, "health.json"), health)


def staged_candidate(state_dir: Path, source: dict) -> tuple[Path, dict, dict]:
    state = private_root(state_dir, create=False)
    if not state.is_dir():
        raise SyncError("missing_staged_catalog")
    # Resolve the one systemd-owned private state alias before rejecting links.
    state = no_links(state)
    state_catalog = load(child(state, "catalog.json"))
    row = state_catalog.get("sources", {}).get(source["id"], {})
    candidate = row.get("candidate", {})
    if row.get("status") != "downloaded_unverified" or not candidate or row.get("metadata") != candidate.get("metadata"):
        raise SyncError("stale_or_unavailable_candidate")
    sha256 = candidate.get("sha256")
    metadata = candidate.get("metadata", {})
    if not isinstance(sha256, str) or not re.fullmatch(r"[a-f0-9]{64}", sha256) or type(metadata.get("size")) is not int or not 0 < metadata["size"] <= MAX_BYTES:
        raise SyncError("invalid_staged_candidate")
    path = candidate_path(state, candidate)
    if path.name != f"{sha256}.{source['format']}" or path.stat().st_size != metadata["size"] or digest(path) != sha256:
        raise SyncError("staged_object_corrupt")
    package = inspect_package(path, source)
    if package.get("identity") != source["identity"]:
        if source["format"] == "dmg" and package.get("version") == "unknown":
            package["requiresNativeReceipt"] = True
        else:
            raise SyncError("native_verification_required")
    if source["platform"] == "windows" and package.get("publisher") != publisher(source):
        raise SyncError("native_identity_mismatch")
    if package.get("version") == "unknown":
        # A fixed-name DMG does not expose trustworthy identity/version without
        # native inspection. It therefore still needs a matching receipt.
        package["requiresNativeReceipt"] = True
    return state, candidate, package


def publish(state_dir: Path, output_dir: Path, source_id: str, receipt_file: Path | None = None) -> dict:
    configured = {s["id"]: s for s in sources()}
    source = configured.get(source_id)
    if source is None:
        raise SyncError("unknown_source")
    receipt = load(receipt_file, operator=True) if receipt_file is not None else None
    root = public_root(output_dir)
    with locked(root):
        old_catalog = catalog(root, configured)
        old = next((row for row in old_catalog["artifacts"] if row["sourceId"] == source_id), None)
        if old and receipt is not None and receipt.get("sha256") == old["sha256"]:
            # A matching native receipt can replay an already authoritative
            # immutable object even if private discovery later becomes stale.
            package = inspect_package(artifact_path(root, old), source)
            native = verified_for(receipt, source, old["sha256"], package)
            if native["version"] != old["version"]:
                raise SyncError("immutable_metadata_conflict")
            reconcile_health(root, old_catalog)
            return {"sourceId": source_id, "sha256": old["sha256"], "published": True, "reused": True, "verification": old["verification"]}
        state, candidate, package = staged_candidate(state_dir, source)
        sha256 = candidate.get("sha256")
        metadata = candidate.get("metadata", {})
        path = candidate_path(state, candidate)
        if old and old["sha256"] == sha256:
            reconcile_health(root, old_catalog)
            return {"sourceId": source_id, "sha256": sha256, "published": True, "reused": True, "verification": old["verification"]}
        if receipt is None:
            if package.pop("requiresNativeReceipt", False):
                raise SyncError("native_verification_required")
            native = package
            verification = "client_native_required"
            checked_at = utc()
        else:
            native = verified_for(receipt, source, sha256, package)
            verification = "native_verified"
            checked_at = receipt["checkedAt"]
        artifact = {"sourceId": source_id, **{key: source[key] for key in ("app", "platform", "architecture", "format", "identity")},
                    "version": native["version"], "url": f"{ORIGIN}/apps/{source_id}/{sha256}.{source['format']}",
                    "sha256": sha256, "size": metadata["size"], "originUrl": metadata.get("url"),
                    "publisher": publisher(source), "verifiedAt": checked_at, "verification": verification}
        validate_artifact(artifact, configured, 2)
        if old and version_tuple(artifact["version"]) < version_tuple(old["version"]):
            raise SyncError("version_regression")
        if old and old["sha256"] == sha256 and any(old[key] != artifact[key] for key in ARTIFACT_KEYS - {"verifiedAt"}):
            raise SyncError("immutable_metadata_conflict")
        # Re-read immediately before copying: a newer scan must not silently
        # turn this explicit source selection into a different package.
        if load(child(state, "catalog.json")).get("sources", {}).get(source_id, {}).get("candidate") != candidate:
            raise SyncError("staged_candidate_changed")
        directory(child(root, f"apps/{source_id}"))
        copy_immutable(path, artifact_path(root, artifact), sha256, artifact["size"])
        current = load(child(state, "catalog.json")).get("sources", {}).get(source_id, {})
        if current.get("candidate") != candidate or current.get("metadata") != metadata or current.get("status") != "downloaded_unverified":
            raise SyncError("staged_candidate_changed")
        if old and old["sha256"] == sha256:
            updated = old_catalog
        else:
            rows = [row for row in old_catalog["artifacts"] if row["sourceId"] != source_id] + [artifact]
            updated = {"schemaVersion": 2, "generatedAt": utc(), "artifacts": sorted(rows, key=lambda item: item["sourceId"])}
            atomic_public_json(child(root, "apps/catalog.json"), updated)
        # Catalog is authoritative. A crash here may leave an under-reporting
        # health file; an identical replay repairs health without republishing.
        reconcile_health(root, updated)
        return {"sourceId": source_id, "sha256": sha256, "published": True, "reused": False, "verification": verification}


def disable(output_dir: Path) -> dict:
    configured = {s["id"]: s for s in sources()}
    root = public_root(output_dir)
    with locked(root):
        previous = catalog(root, configured)
        disabled = {"schemaVersion": 2, "generatedAt": utc(), "artifacts": []}
        atomic_public_json(child(root, "apps/catalog.json"), disabled)
        reconcile_health(root, disabled)
        return {"disabled": True, "previousArtifactCount": len(previous["artifacts"]), "objectsDeleted": False}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    actions = parser.add_subparsers(dest="action", required=True)
    publish_parser = actions.add_parser("publish", help="publish one current official-origin candidate")
    publish_parser.add_argument("--state-dir", type=Path, required=True)
    publish_parser.add_argument("--public-root", type=Path, required=True)
    publish_parser.add_argument("--source", required=True)
    publish_parser.add_argument("--verification", type=Path)
    disable_parser = actions.add_parser("disable", help="atomically disable every mirror slot")
    disable_parser.add_argument("--public-root", type=Path, required=True)
    prune_parser = actions.add_parser("prune", help="keep the current and one previous public object per source")
    prune_parser.add_argument("--public-root", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.action == "publish":
            result = publish(args.state_dir, args.public_root, args.source, args.verification)
        elif args.action == "prune":
            result = prune_history(args.public_root)
        else:
            result = disable(args.public_root)
        print(json.dumps(result))
        return 0
    except (SyncError, OSError, ValueError, KeyError, TypeError) as exc:
        # A process failure after rename or before health update cannot truthfully
        # assert that no catalog was published. Replay reconciles this boundary.
        print(json.dumps({"publication": "incomplete", "error": str(exc) if isinstance(exc, SyncError) else "publisher_local_error", "replayRequired": True}))
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
