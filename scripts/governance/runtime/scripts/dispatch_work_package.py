#!/usr/bin/env python3
"""Bind a frozen work-package definition to an auditable execution run."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import subprocess
import sys
from typing import Any

from execution_integrity import (
    baseline_relative_path,
    bundle_execution_path,
    expected_execution_identity,
    run_execution_identity,
    sha256,
    validate_completed_run_integrity,
)
from governance_common import BUNDLE_DIR, read_object, source_tree_snapshot, write_object
from release_units import (
    frozen_release_unit_superseders,
    load_release_unit_snapshot,
    release_unit_index,
    snapshot_package,
)
from validate_governance import paths_overlap, validate_bundle


RUNS_PATH = Path("execution/work-package-runs.json")
SAFE_ASSIGNEE = re.compile(r"^[A-Za-z0-9._/@:-]{1,120}$")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project-root", required=True, type=Path)
    parser.add_argument("--package-id", required=True)
    parser.add_argument("--assigned-to", required=True)
    parser.add_argument(
        "--release-unit",
        help="Dispatch from a frozen implementation release unit.",
    )
    return parser.parse_args()


def git_head(root: Path) -> str:
    result = subprocess.run(
        ["git", "-C", str(root), "rev-parse", "HEAD"],
        check=False,
        capture_output=True,
        text=True,
        timeout=20,
    )
    if result.returncode != 0 or not result.stdout.strip():
        raise ValueError(result.stderr.strip() or "Cannot resolve Git HEAD")
    return result.stdout.strip()


def load_package(
    root: Path,
    package_id: str,
    release_unit_id: str | None = None,
    *,
    allow_superseded: bool = False,
) -> tuple[dict[str, Any], dict[str, Any], dict[str, str]]:
    bundle = root / BUNDLE_DIR
    manifest = read_object(bundle / "manifest.json")
    if release_unit_id is not None:
        release_path = manifest.get("artifacts", {}).get("releaseUnits")
        if not isinstance(release_path, str):
            raise ValueError("manifest.artifacts.releaseUnits is missing")
        units = release_unit_index(read_object(bundle / release_path))
        unit = units.get(release_unit_id)
        if unit is None or unit.get("status") != "frozen":
            raise ValueError(f"Release unit {release_unit_id} is not frozen")
        superseders = frozen_release_unit_superseders(units, release_unit_id)
        if superseders and not allow_superseded:
            raise ValueError(
                f"Release unit {release_unit_id} is superseded by {superseders}; "
                "new dispatch is forbidden"
            )
        snapshot = load_release_unit_snapshot(root, unit)
        package = snapshot_package(snapshot, package_id)
        identity = expected_execution_identity(
            None,
            release_unit_id=release_unit_id,
            release_unit_hash=str(snapshot["releaseUnitHash"]),
        )
        return manifest, package, identity
    relative = manifest["artifacts"]["workPackages"]
    data = read_object(bundle / relative)
    packages = data.get("packages")
    if not isinstance(packages, list):
        raise ValueError("work-packages.json packages must be an array")
    package = next(
        (item for item in packages if isinstance(item, dict) and item.get("id") == package_id),
        None,
    )
    if package is None:
        raise ValueError(f"Unknown work package: {package_id}")
    return manifest, package, expected_execution_identity(
        str(manifest["freeze"]["manifestHash"])
    )


def load_package_for_run(
    root: Path, run: dict[str, Any]
) -> tuple[dict[str, Any], dict[str, Any], dict[str, str]]:
    identity = run_execution_identity(run)
    return load_package(
        root,
        str(run.get("packageId")),
        identity.get("releaseUnitId"),
        allow_superseded=True,
    )


def load_dependency_package(
    root: Path,
    package_id: str,
    current_release_unit_id: str | None,
    current_identity: dict[str, str],
) -> tuple[dict[str, Any], dict[str, str]]:
    """Resolve an intra-unit or externally satisfied frozen package dependency."""

    if current_release_unit_id is None:
        _, package, identity = load_package(root, package_id)
        return package, identity

    bundle = root / BUNDLE_DIR
    manifest = read_object(bundle / "manifest.json")
    release_path = manifest.get("artifacts", {}).get("releaseUnits")
    if not isinstance(release_path, str):
        raise ValueError("manifest.artifacts.releaseUnits is missing")
    units = release_unit_index(read_object(bundle / release_path))
    current_unit = units.get(current_release_unit_id)
    if current_unit is None or current_unit.get("status") != "frozen":
        raise ValueError(f"Release unit {current_release_unit_id} is not frozen")
    current_snapshot = load_release_unit_snapshot(root, current_unit)
    if current_snapshot.get("releaseUnitHash") != current_identity.get("releaseUnitHash"):
        raise ValueError("Current release-unit snapshot hash changed after dispatch")
    current_packages = (
        current_snapshot.get("artifacts", {})
        .get("workPackages", {})
        .get("packages", [])
    )
    if not isinstance(current_packages, list):
        raise ValueError("Current release-unit work packages are malformed")
    current_match = next(
        (
            item
            for item in current_packages
            if isinstance(item, dict) and item.get("id") == package_id
        ),
        None,
    )
    if current_match is not None:
        return current_match, current_identity

    matches: list[tuple[dict[str, Any], dict[str, str]]] = []
    for dependency_unit_id in current_snapshot.get("dependsOnReleaseUnits", []):
        dependency_unit = units.get(dependency_unit_id)
        if dependency_unit is None or dependency_unit.get("status") != "frozen":
            raise ValueError(
                f"Dependency release unit {dependency_unit_id} is not frozen"
            )
        dependency_snapshot = load_release_unit_snapshot(root, dependency_unit)
        dependency_packages = (
            dependency_snapshot.get("artifacts", {})
            .get("workPackages", {})
            .get("packages", [])
        )
        if not isinstance(dependency_packages, list):
            raise ValueError(
                f"Dependency release unit {dependency_unit_id} work packages are malformed"
            )
        dependency_match = next(
            (
                item
                for item in dependency_packages
                if isinstance(item, dict) and item.get("id") == package_id
            ),
            None,
        )
        if dependency_match is not None:
            matches.append(
                (
                    dependency_match,
                    expected_execution_identity(
                        None,
                        release_unit_id=dependency_unit_id,
                        release_unit_hash=str(
                            dependency_snapshot["releaseUnitHash"]
                        ),
                    ),
                )
            )
    if not matches:
        raise ValueError(
            f"Dependency package {package_id} is absent from the current and declared dependency units"
        )
    if len(matches) != 1:
        raise ValueError(
            f"Dependency package {package_id} is supplied by multiple dependency units"
        )
    return matches[0]


def completed_run_is_verified(
    root: Path,
    run: dict[str, Any],
    package: dict[str, Any],
    manifest_hash: str | None,
    *,
    release_unit_id: str | None = None,
    release_unit_hash: str | None = None,
) -> bool:
    return not validate_completed_run_integrity(
        root,
        run,
        package,
        manifest_hash if release_unit_id is None else None,
        check_current_outputs=True,
        release_unit_id=release_unit_id,
        release_unit_hash=release_unit_hash,
    )


def main() -> int:
    args = parse_args()
    root = args.project_root.expanduser().resolve()
    if not SAFE_ASSIGNEE.fullmatch(args.assigned_to):
        print("assigned-to contains unsupported characters", file=sys.stderr)
        return 2
    validation = validate_bundle(root, "freeze", args.release_unit)
    if validation.errors:
        print("Cannot dispatch from an invalid or unfrozen bundle", file=sys.stderr)
        for error in validation.errors:
            print(f"ERROR: {error}", file=sys.stderr)
        return 3
    try:
        manifest, package, identity = load_package(
            root, args.package_id, args.release_unit
        )
        if package.get("status") != "planned":
            raise ValueError(f"Work package {args.package_id} is not dispatchable")
        runs_path = root / BUNDLE_DIR / RUNS_PATH
        registry = read_object(runs_path)
        runs = registry.get("runs")
        if not isinstance(runs, list):
            raise ValueError("Execution registry runs must be an array")
        current_runs = [
            run
            for run in runs
            if isinstance(run, dict)
            and run_execution_identity(run) == identity
        ]
        dependencies = package.get("dependsOn", [])
        for dependency in dependencies:
            dependency_package, dependency_identity = load_dependency_package(
                root,
                dependency,
                args.release_unit,
                identity,
            )
            latest_dependency = next((
                run for run in reversed(runs)
                if isinstance(run, dict)
                and run.get("packageId") == dependency
                and run_execution_identity(run) == dependency_identity
            ), None)
            if not (
                latest_dependency is not None
                and latest_dependency.get("status") == "completed"
                and completed_run_is_verified(
                    root,
                    latest_dependency,
                    dependency_package,
                    dependency_identity.get("manifestHash"),
                    release_unit_id=dependency_identity.get("releaseUnitId"),
                    release_unit_hash=dependency_identity.get("releaseUnitHash"),
                )
            ):
                raise ValueError(
                    f"Dependency {dependency} has no completed run for {dependency_identity}"
                )
        if any(
            run.get("packageId") == args.package_id and run.get("status") == "in_progress"
            for run in current_runs
        ):
            raise ValueError(f"Work package {args.package_id} already has an active run")
        active_runs = [
            run
            for run in runs
            if isinstance(run, dict) and run.get("status") == "in_progress"
        ]
        for active in active_runs:
            if active in current_runs and active.get("packageId") == args.package_id:
                continue
            _, active_package, _ = load_package_for_run(root, active)
            for own_path in package.get("allowedPaths", []):
                for active_path in active_package.get("allowedPaths", []):
                    if paths_overlap(own_path, active_path):
                        raise ValueError(
                            f"Path ownership overlaps active run {active.get('id')}: "
                            f"{own_path} vs {active_path}"
                        )
        started_at = datetime.now(timezone.utc).isoformat()
        prefix = f"{args.release_unit}/" if args.release_unit else ""
        run_id = f"{prefix}{args.package_id}@{started_at}"
        baseline_snapshot = source_tree_snapshot(root)
        if args.release_unit:
            units = release_unit_index(read_object(root / BUNDLE_DIR / manifest["artifacts"]["releaseUnits"]))
            authority = load_release_unit_snapshot(root, units[args.release_unit])
            binding = authority.get("baselineReconciliation")
            readonly_unit = all(p.get("verificationOnly") is True for p in authority["artifacts"]["workPackages"]["packages"])
            if binding is not None and (not current_runs or readonly_unit):
                from baseline_reconciliation import validate_current_receipt
                validate_current_receipt(root, binding, baseline_snapshot)
        baseline_relative = baseline_relative_path(run_id, args.package_id)
        baseline_path = bundle_execution_path(root, baseline_relative)
        write_object(baseline_path, baseline_snapshot)
        run = {
                "id": run_id,
                "packageId": args.package_id,
                "baselineCommit": git_head(root),
                "baselineTreeHash": baseline_snapshot["treeHash"],
                "baselineSnapshotPath": baseline_relative,
                "baselineSnapshotSha256": sha256(baseline_path),
                "assignedTo": args.assigned_to,
                "status": "in_progress",
                "startedAt": started_at,
                "finishedAt": None,
                "reportPath": None,
                "reportSha256": None,
            }
        run.update(identity)
        runs.append(run)
        write_object(runs_path, registry)
    except (KeyError, OSError, ValueError, json.JSONDecodeError, subprocess.TimeoutExpired) as exc:
        print(str(exc), file=sys.stderr)
        return 4
    print(run_id)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
