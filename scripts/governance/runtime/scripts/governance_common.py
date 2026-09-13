"""Shared deterministic helpers for product governance scripts."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
from typing import Any


BUNDLE_DIR = ".product-governance"


def read_object(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        value = json.load(handle)
    if not isinstance(value, dict):
        raise ValueError(f"Expected a JSON object: {path}")
    return value


def write_object(path: Path, value: object) -> None:
    rendered = json.dumps(value, ensure_ascii=False, indent=2, sort_keys=False) + "\n"
    path.parent.mkdir(parents=True, exist_ok=True)
    file_descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{path.name}.", suffix=".tmp", dir=path.parent
    )
    try:
        with os.fdopen(file_descriptor, "w", encoding="utf-8") as handle:
            handle.write(rendered)
            handle.flush()
            os.fsync(handle.fileno())
        Path(temporary_name).replace(path)
    except BaseException:
        try:
            Path(temporary_name).unlink(missing_ok=True)
        finally:
            raise


def canonical_bytes(value: object) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")


def governance_hash(bundle: Path, manifest: dict[str, Any] | None = None) -> str:
    current_manifest = manifest or read_object(bundle / "manifest.json")
    normalized_manifest = json.loads(json.dumps(current_manifest))
    freeze = normalized_manifest.get("freeze")
    if isinstance(freeze, dict):
        freeze["manifestHash"] = None
    digest = hashlib.sha256()
    digest.update(b"manifest.json\0")
    digest.update(canonical_bytes(normalized_manifest))
    artifacts = current_manifest.get("artifacts")
    if not isinstance(artifacts, dict):
        raise ValueError("manifest.artifacts must be an object")
    for relative_path in sorted({str(value) for value in artifacts.values()}):
        path = bundle / relative_path
        digest.update(relative_path.encode("utf-8") + b"\0")
        digest.update(canonical_bytes(read_object(path)))
    return f"sha256:{digest.hexdigest()}"


def source_tree_snapshot(root: Path) -> dict[str, Any]:
    """Hash every tracked or non-ignored untracked project file.

    Git's own ignore rules keep dependency caches and generated build output
    outside the evidence boundary. Governance execution journals are excluded
    explicitly because the verifier itself mutates them while preserving the
    frozen governance artifacts.
    """
    result = subprocess.run(
        ["git", "-C", str(root), "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        check=False,
        capture_output=True,
        timeout=60,
    )
    if result.returncode != 0:
        raise ValueError(result.stderr.decode("utf-8", errors="replace").strip() or "Cannot enumerate Git source tree")
    paths = sorted(
        {
            raw.decode("utf-8")
            for raw in result.stdout.split(b"\0")
            if raw
        }
    )
    hashes: dict[str, str] = {}
    for relative in paths:
        parts = Path(relative).parts
        if (
            any(part in {"__pycache__", ".pytest_cache", ".mypy_cache", ".ruff_cache"} for part in parts)
            or relative.endswith((".pyc", ".pyo"))
            or Path(relative).name in {".coverage"}
        ):
            continue
        if relative == f"{BUNDLE_DIR}/execution" or relative.startswith(
            f"{BUNDLE_DIR}/execution/"
        ):
            continue
        path = root / relative
        if path.is_symlink():
            payload = os.readlink(path).encode("utf-8")
            hashes[relative] = f"symlink-sha256:{hashlib.sha256(payload).hexdigest()}"
        elif path.is_file():
            hashes[relative] = f"sha256:{hashlib.sha256(path.read_bytes()).hexdigest()}"
        else:
            hashes[relative] = "missing"
    return {
        "treeHash": f"sha256:{hashlib.sha256(canonical_bytes(hashes)).hexdigest()}",
        "pathHashes": hashes,
    }
