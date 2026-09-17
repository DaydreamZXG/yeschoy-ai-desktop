from __future__ import annotations

import hashlib
import json
import os
import re
import stat
import tempfile
from pathlib import Path
from typing import Any


MAX_METADATA_BYTES = 256 * 1024
MAX_ARTIFACT_BYTES = 4 * 1024 * 1024 * 1024
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
GIT_OBJECT_RE = re.compile(r"^[0-9a-f]{40}$")
SAFE_NAME_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._+-]{0,179}$")
TARGETS = ("win-x64", "mac-arm64", "mac-x64")
PLACEHOLDER_PARTS = (
    "change-me",
    "changeme",
    "example",
    "placeholder",
    "replace-me",
    "tbd",
    "todo",
    "unknown",
)


class PolicyError(RuntimeError):
    def __init__(self, code: str):
        super().__init__(code)
        self.code = code


def fail(code: str) -> None:
    raise PolicyError(code)


def _closed_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            fail("duplicate_json_key")
        result[key] = value
    return result


def read_regular_bytes(path: Path, maximum: int, *, single_link: bool = True) -> bytes:
    try:
        if path.is_symlink():
            fail("symlink_not_allowed")
        flags = os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NOFOLLOW", 0)
        descriptor = os.open(path, flags)
    except PolicyError:
        raise
    except OSError:
        fail("file_unavailable")
    try:
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode):
            fail("regular_file_required")
        if single_link and info.st_nlink != 1:
            fail("hardlink_not_allowed")
        if info.st_size < 0 or info.st_size > maximum:
            fail("file_size_out_of_bounds")
        chunks: list[bytes] = []
        remaining = maximum + 1
        while remaining > 0:
            chunk = os.read(descriptor, min(1024 * 1024, remaining))
            if not chunk:
                break
            chunks.append(chunk)
            remaining -= len(chunk)
        data = b"".join(chunks)
        if len(data) > maximum:
            fail("file_size_out_of_bounds")
        return data
    finally:
        os.close(descriptor)


def load_json(path: Path) -> tuple[dict[str, Any], bytes]:
    raw = read_regular_bytes(path, MAX_METADATA_BYTES)
    try:
        value = json.loads(raw.decode("utf-8"), object_pairs_hook=_closed_object)
    except (UnicodeDecodeError, json.JSONDecodeError):
        fail("invalid_json")
    if not isinstance(value, dict):
        fail("json_object_required")
    return value, raw


def require_exact_keys(value: dict[str, Any], expected: set[str], code: str) -> None:
    if set(value) != expected:
        fail(code)


def _require_string(value: Any, code: str) -> str:
    if not isinstance(value, str) or not value:
        fail(code)
    return value


def validate_spec(value: dict[str, Any]) -> dict[str, Any]:
    require_exact_keys(
        value,
        {
            "schemaVersion",
            "policyId",
            "upstreamRepository",
            "upstreamCommit",
            "upstreamTree",
            "rootVersion",
            "desktopVersion",
            "appId",
            "license",
            "targets",
            "signingPolicy",
        },
        "invalid_spec_fields",
    )
    if value["schemaVersion"] != 1:
        fail("unsupported_spec_schema")
    for key in (
        "policyId",
        "upstreamRepository",
        "upstreamCommit",
        "upstreamTree",
        "rootVersion",
        "desktopVersion",
        "appId",
    ):
        _require_string(value[key], f"invalid_spec_{key}")
    if not GIT_OBJECT_RE.fullmatch(value["upstreamCommit"]):
        fail("invalid_spec_commit")
    if not GIT_OBJECT_RE.fullmatch(value["upstreamTree"]):
        fail("invalid_spec_tree")

    license_value = value["license"]
    if not isinstance(license_value, dict):
        fail("invalid_spec_license")
    require_exact_keys(license_value, {"spdx", "sha256"}, "invalid_spec_license_fields")
    if license_value["spdx"] != "MIT" or not SHA256_RE.fullmatch(
        _require_string(license_value["sha256"], "invalid_spec_license_hash")
    ):
        fail("invalid_spec_license")

    targets = value["targets"]
    policies = value["signingPolicy"]
    if not isinstance(targets, dict) or set(targets) != set(TARGETS):
        fail("invalid_spec_targets")
    if not isinstance(policies, dict) or set(policies) != set(TARGETS):
        fail("invalid_spec_signing_policy")
    for target in TARGETS:
        target_value = targets[target]
        if not isinstance(target_value, dict):
            fail("invalid_spec_target")
        require_exact_keys(
            target_value,
            {
                "command",
                "artifactExtension",
                "signatureKind",
                "verificationTool",
                "requiresNotarization",
            },
            "invalid_spec_target_fields",
        )
        command = target_value["command"]
        if (
            not isinstance(command, list)
            or not command
            or not all(isinstance(part, str) and part for part in command)
        ):
            fail("invalid_spec_target_command")
        extension = target_value["artifactExtension"]
        if extension not in {".exe", ".dmg"}:
            fail("invalid_spec_artifact_extension")
        if target == "win-x64":
            expected = ("windows-authenticode", "signtool", False, ".exe")
        else:
            expected = ("apple-developer-id-notarized", "codesign-notarytool", True, ".dmg")
        observed = (
            target_value["signatureKind"],
            target_value["verificationTool"],
            target_value["requiresNotarization"],
            extension,
        )
        if observed != expected:
            fail("invalid_spec_target_security_policy")

        policy = policies[target]
        if not isinstance(policy, dict):
            fail("invalid_spec_signer")
        require_exact_keys(policy, {"publisherId"}, "invalid_spec_signer_fields")
        publisher = policy["publisherId"]
        if publisher is not None and not isinstance(publisher, str):
            fail("invalid_spec_publisher")
    return value


def load_spec(path: Path, *, allow_fixture: bool = False) -> tuple[dict[str, Any], bytes]:
    shipped = Path(__file__).resolve().with_name("build-spec.json")
    is_fixture = allow_fixture and os.environ.get("YESCHOY_DSH_SOURCE_BUILD_TESTING") == "1"
    try:
        selected = path.resolve(strict=True)
    except OSError:
        fail("spec_unavailable")
    if selected != shipped and not is_fixture:
        fail("untrusted_spec_path")
    value, raw = load_json(selected)
    return validate_spec(value), raw


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path, maximum: int = MAX_ARTIFACT_BYTES) -> tuple[str, int]:
    try:
        if path.is_symlink():
            fail("symlink_not_allowed")
        flags = os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NOFOLLOW", 0)
        descriptor = os.open(path, flags)
    except PolicyError:
        raise
    except OSError:
        fail("file_unavailable")
    try:
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode):
            fail("regular_file_required")
        if info.st_nlink != 1:
            fail("hardlink_not_allowed")
        if info.st_size < 1:
            fail("empty_artifact")
        if info.st_size > maximum:
            fail("file_size_out_of_bounds")
        digest = hashlib.sha256()
        observed = 0
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            if not chunk:
                break
            observed += len(chunk)
            if observed > maximum:
                fail("file_size_out_of_bounds")
            digest.update(chunk)
        if observed != info.st_size:
            fail("artifact_changed_during_read")
        return digest.hexdigest(), observed
    finally:
        os.close(descriptor)


def validate_publisher(value: Any) -> str:
    if not isinstance(value, str) or not (3 <= len(value) <= 240):
        fail("signing_policy_unconfigured")
    folded = value.casefold()
    if any(part in folded for part in PLACEHOLDER_PARTS):
        fail("signing_policy_unconfigured")
    return value


def canonical_json(value: dict[str, Any]) -> bytes:
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n").encode(
        "utf-8"
    )


def atomic_write_json(path: Path, value: dict[str, Any], *, idempotent: bool = False) -> None:
    encoded = canonical_json(value)
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.is_symlink():
        fail("output_symlink_not_allowed")
    if path.exists():
        existing = read_regular_bytes(path, MAX_METADATA_BYTES)
        if idempotent and existing == encoded:
            return
        fail("output_conflict")
    descriptor, temporary_name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    temporary = Path(temporary_name)
    try:
        os.fchmod(descriptor, 0o600)
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(encoded)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    except Exception:
        temporary.unlink(missing_ok=True)
        raise
