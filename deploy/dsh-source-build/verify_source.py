#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import Path

from policy import (
    MAX_METADATA_BYTES,
    PolicyError,
    atomic_write_json,
    load_json,
    load_spec,
    read_regular_bytes,
    sha256_bytes,
)


def git(source: Path, *arguments: str) -> str:
    environment = {
        **os.environ,
        "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_TERMINAL_PROMPT": "0",
        "LC_ALL": "C",
    }
    result = subprocess.run(
        ["git", "-C", str(source), *arguments],
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        env=environment,
        timeout=60,
    )
    if result.returncode != 0:
        raise PolicyError("git_verification_failed")
    return result.stdout.strip()


def package_version(path: Path) -> str:
    value, _ = load_json(path)
    version = value.get("version")
    if not isinstance(version, str) or not version:
        raise PolicyError("package_version_missing")
    return version


def verify_source(source: Path, spec: dict, spec_raw: bytes) -> dict:
    if source.is_symlink() or not source.is_dir():
        raise PolicyError("source_boundary_invalid")
    git_boundary = source / ".git"
    if git_boundary.is_symlink() or not git_boundary.exists():
        raise PolicyError("source_git_boundary_invalid")

    repository = git(source, "remote", "get-url", "origin")
    if repository != spec["upstreamRepository"]:
        raise PolicyError("upstream_repository_mismatch")
    commit = git(source, "rev-parse", "--verify", "HEAD^{commit}")
    if commit != spec["upstreamCommit"]:
        raise PolicyError("upstream_commit_mismatch")
    tree = git(source, "rev-parse", "--verify", "HEAD^{tree}")
    if tree != spec["upstreamTree"]:
        raise PolicyError("upstream_tree_mismatch")
    if git(source, "status", "--porcelain=v1", "--untracked-files=no"):
        raise PolicyError("tracked_checkout_dirty")

    submodules = git(source, "submodule", "status", "--recursive")
    for line in submodules.splitlines():
        if line and not line.startswith(" "):
            raise PolicyError("submodule_state_invalid")

    root_version = package_version(source / "package.json")
    desktop_version = package_version(source / "apps" / "desktop" / "package.json")
    if root_version != spec["rootVersion"] or desktop_version != spec["desktopVersion"]:
        raise PolicyError("package_version_mismatch")

    license_bytes = read_regular_bytes(source / "LICENSE", MAX_METADATA_BYTES)
    license_sha256 = sha256_bytes(license_bytes)
    if license_sha256 != spec["license"]["sha256"]:
        raise PolicyError("license_digest_mismatch")

    return {
        "schemaVersion": 1,
        "status": "verified",
        "policyId": spec["policyId"],
        "specSha256": sha256_bytes(spec_raw),
        "upstreamRepository": repository,
        "upstreamCommit": commit,
        "upstreamTree": tree,
        "rootVersion": root_version,
        "desktopVersion": desktop_version,
        "appId": spec["appId"],
        "licenseSpdx": spec["license"]["spdx"],
        "licenseSha256": license_sha256,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify one pinned DeepSeek Harness checkout.")
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--spec", type=Path, default=Path(__file__).with_name("build-spec.json"))
    parser.add_argument("--receipt", required=True, type=Path)
    parser.add_argument("--allow-fixture-spec", action="store_true", help=argparse.SUPPRESS)
    arguments = parser.parse_args()
    try:
        spec, spec_raw = load_spec(arguments.spec, allow_fixture=arguments.allow_fixture_spec)
        receipt = verify_source(arguments.source.absolute(), spec, spec_raw)
        atomic_write_json(arguments.receipt.absolute(), receipt)
        print("source_verified")
        return 0
    except (OSError, subprocess.SubprocessError, PolicyError) as error:
        code = error.code if isinstance(error, PolicyError) else "source_verification_failed"
        print(code, file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
