#!/usr/bin/env python3
"""Collect repository evidence about competing authorities and implementation state."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from typing import Any, Iterable


EXCLUDED = {
    ".git",
    ".product-governance",
    "node_modules",
    ".venv",
    "venv",
    "dist",
    "build",
    ".next",
    "coverage",
}
AUTHORITY_PATTERNS = (
    re.compile(r"唯一(?:执行)?权威"),
    re.compile(r"执行基线"),
    re.compile(r"设计基线"),
    re.compile(r"目标架构"),
    re.compile(r"source\s+of\s+truth", re.IGNORECASE),
    re.compile(r"authoritative", re.IGNORECASE),
)
COMPLETION_PATTERN = re.compile(r"(?:已完成|complete(?:d)?|production[- ]ready)", re.IGNORECASE)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project-root", required=True, type=Path)
    parser.add_argument("--output", type=Path)
    return parser.parse_args()


def iter_files(root: Path, suffixes: set[str]) -> Iterable[Path]:
    for current_root, directory_names, file_names in os.walk(root):
        directory_names[:] = sorted(
            name for name in directory_names if name not in EXCLUDED
        )
        current = Path(current_root)
        for file_name in sorted(file_names):
            path = current / file_name
            if path.suffix.lower() not in suffixes:
                continue
            try:
                if path.stat().st_size > 2_000_000:
                    continue
            except OSError:
                continue
            yield path


def relative(path: Path, root: Path) -> str:
    return path.relative_to(root).as_posix()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65_536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def text_claims(path: Path, root: Path) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    authorities: list[dict[str, Any]] = []
    completions: list[dict[str, Any]] = []
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return authorities, completions
    for line_number, line in enumerate(lines, start=1):
        excerpt = line.strip()
        if not excerpt:
            continue
        if any(pattern.search(excerpt) for pattern in AUTHORITY_PATTERNS):
            authorities.append(
                {"locator": f"{relative(path, root)}:{line_number}", "excerpt": excerpt[:500]}
            )
        if COMPLETION_PATTERN.search(excerpt):
            completions.append(
                {"locator": f"{relative(path, root)}:{line_number}", "excerpt": excerpt[:500]}
            )
    return authorities, completions


def git_evidence(root: Path) -> dict[str, Any]:
    try:
        result = subprocess.run(
            ["git", "-C", str(root), "status", "--porcelain=v1"],
            check=False,
            capture_output=True,
            text=True,
            timeout=20,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        return {"available": False, "error": str(exc)}
    if result.returncode != 0:
        return {"available": False, "error": result.stderr.strip()}
    entries = [line for line in result.stdout.splitlines() if line]
    head = subprocess.run(
        ["git", "-C", str(root), "rev-parse", "HEAD"],
        check=False,
        capture_output=True,
        text=True,
        timeout=20,
    )
    branch = subprocess.run(
        ["git", "-C", str(root), "branch", "--show-current"],
        check=False,
        capture_output=True,
        text=True,
        timeout=20,
    )
    return {
        "available": True,
        "head": head.stdout.strip() if head.returncode == 0 else None,
        "branch": branch.stdout.strip() if branch.returncode == 0 else None,
        "dirty": bool(entries),
        "changedEntryCount": len(entries),
        "entries": entries,
    }


def main() -> int:
    args = parse_args()
    root = args.project_root.expanduser().resolve()
    if not root.is_dir():
        print(f"Project root does not exist: {root}", file=sys.stderr)
        return 2

    authority_claims: list[dict[str, Any]] = []
    completion_claims: list[dict[str, Any]] = []
    documents: list[dict[str, Any]] = []
    for path in sorted(iter_files(root, {".md", ".mdx", ".txt"})):
        authorities, completions = text_claims(path, root)
        authority_claims.extend(authorities)
        completion_claims.extend(completions)
        if authorities or completions:
            documents.append(
                {
                    "path": relative(path, root),
                    "sha256": sha256(path),
                    "authorityClaimCount": len(authorities),
                    "completionClaimCount": len(completions),
                }
            )

    instruction_paths = [
        relative(path, root)
        for path in sorted(iter_files(root, {".md"}))
        if path.name in {"AGENTS.md", "CLAUDE.md", "CONTRIBUTING.md"}
    ]
    app_manifests = [
        relative(path, root)
        for path in sorted(iter_files(root, {".json"}))
        if path.name in {"app.json", "package.json", "project.config.json"}
    ]
    report = {
        "schemaVersion": 1,
        "observedAt": datetime.now(timezone.utc).isoformat(),
        "projectRoot": str(root),
        "git": git_evidence(root),
        "instructionFiles": instruction_paths,
        "applicationManifests": app_manifests,
        "documentsWithClaims": documents,
        "authorityClaims": authority_claims,
        "completionClaims": completion_claims,
        "findings": {
            "multipleAuthorityClaimsObserved": len(authority_claims) > 1,
            "authorityClaimCount": len(authority_claims),
            "completionClaimCount": len(completion_claims),
        },
        "interpretation": "Observed strings are evidence candidates, not proof of authority or completion.",
    }
    rendered = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        output = args.output.expanduser()
        if not output.is_absolute():
            output = root / output
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(rendered, encoding="utf-8")
        print(output)
    else:
        sys.stdout.write(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
