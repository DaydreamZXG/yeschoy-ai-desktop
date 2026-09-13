#!/usr/bin/env python3
"""Atomically seal a review-ready product governance bundle."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from typing import Any

from execution_integrity import bundle_execution_path
from governance_common import BUNDLE_DIR, governance_hash, read_object, write_object
from release_units import (
    project_release_unit_payload,
    release_unit_snapshot_relative,
)
from validate_governance import validate_bundle


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project-root", required=True, type=Path)
    parser.add_argument("--decision-id", required=True)
    parser.add_argument(
        "--release-unit",
        help="Seal one implementation release unit while the full catalog may remain draft.",
    )
    parser.add_argument(
        "--commit",
        default="HEAD",
        help="Git commit or ref that anchors the architecture review (default: HEAD)",
    )
    return parser.parse_args()


def resolve_commit(root: Path, reference: str) -> str:
    result = subprocess.run(
        ["git", "-C", str(root), "rev-parse", "--verify", f"{reference}^{{commit}}"],
        check=False,
        capture_output=True,
        text=True,
        timeout=20,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or f"Unknown Git commit/ref: {reference}"
        raise ValueError(detail)
    commit = result.stdout.strip()
    if not commit:
        raise ValueError(f"Git returned an empty commit for {reference}")
    return commit


def accepted_decision(root: Path, manifest: dict[str, Any], decision_id: str) -> bool:
    artifacts = manifest.get("artifacts")
    if not isinstance(artifacts, dict):
        return False
    relative_path = artifacts.get("decisionRegister")
    if not isinstance(relative_path, str):
        return False
    data = read_object(root / BUNDLE_DIR / relative_path)
    decisions = data.get("decisions")
    if not isinstance(decisions, list):
        return False
    return any(
        isinstance(item, dict)
        and item.get("id") == decision_id
        and item.get("status") == "accepted"
        for item in decisions
    )


def ensure_no_active_runs(root: Path) -> None:
    registry = read_object(
        root / BUNDLE_DIR / "execution" / "work-package-runs.json"
    )
    runs = registry.get("runs")
    if not isinstance(runs, list):
        raise ValueError("Execution registry runs must be an array")
    active = [
        str(run.get("id"))
        for run in runs
        if isinstance(run, dict) and run.get("status") == "in_progress"
    ]
    if active:
        raise ValueError(
            "Cannot refreeze while work-package runs are active: " + ", ".join(active)
        )


def report_payload(
    root: Path, commit: str, manifest_hash: str, decision_id: str
) -> dict[str, Any]:
    validation = validate_bundle(root, "freeze")
    return {
        "schemaVersion": 1,
        "validatedAt": datetime.now(timezone.utc).isoformat(),
        "mode": "freeze",
        "valid": not validation.errors,
        "errorCount": len(validation.errors),
        "warningCount": len(validation.warnings),
        "errors": validation.errors,
        "warnings": validation.warnings,
        "decisionId": decision_id,
        "baselineCommit": commit,
        "manifestHash": manifest_hash,
    }


def create_freeze_snapshot(bundle: Path, manifest: dict[str, Any], manifest_hash: str) -> Path:
    digest = manifest_hash.removeprefix("sha256:")
    snapshot = bundle / "execution" / "freeze-snapshots" / digest
    if snapshot.exists():
        existing = read_object(snapshot / "manifest.json")
        if governance_hash(snapshot, existing) != manifest_hash:
            raise ValueError(f"Freeze snapshot collision or corruption: {snapshot}")
        return snapshot
    snapshot.parent.mkdir(parents=True, exist_ok=True)
    temporary = Path(tempfile.mkdtemp(prefix=f".{digest}.", dir=snapshot.parent))
    try:
        write_object(temporary / "manifest.json", manifest)
        artifacts = manifest.get("artifacts")
        if not isinstance(artifacts, dict):
            raise ValueError("manifest.artifacts must be an object")
        for relative in sorted(set(artifacts.values())):
            if not isinstance(relative, str):
                raise ValueError("Artifact path must be a string")
            write_object(temporary / relative, read_object(bundle / relative))
        if governance_hash(temporary, manifest) != manifest_hash:
            raise ValueError("New freeze snapshot hash does not match the sealed manifest")
        temporary.replace(snapshot)
    except BaseException:
        shutil.rmtree(temporary, ignore_errors=True)
        raise
    return snapshot


def create_release_unit_snapshot(
    bundle: Path, payload: dict[str, Any]
) -> tuple[Path, bool]:
    release_hash = str(payload["releaseUnitHash"])
    relative = release_unit_snapshot_relative(release_hash)
    snapshot = bundle_execution_path(bundle.parent, relative)
    if snapshot.exists():
        existing = read_object(snapshot)
        if existing != payload:
            raise ValueError(f"Release-unit snapshot collision or corruption: {snapshot}")
        return snapshot, False
    snapshot.parent.mkdir(parents=True, exist_ok=True)
    write_object(snapshot, payload)
    return snapshot, True


def seal_release_unit(
    root: Path,
    release_unit_id: str,
    decision_id: str,
    commit_reference: str,
) -> int:
    bundle = root / BUNDLE_DIR
    review = validate_bundle(root, "review", release_unit_id)
    if review.errors or review.release_unit_context is None:
        print("Refusing to seal release unit: review validation failed.", file=sys.stderr)
        for message in review.errors:
            print(f"ERROR: {message}", file=sys.stderr)
        return 3
    context = review.release_unit_context
    unit = context["unit"]
    manifest = context["manifest"]
    if unit.get("decisionRef") != decision_id:
        print(
            f"Release unit {release_unit_id} is bound to decision {unit.get('decisionRef')}, "
            f"not {decision_id}",
            file=sys.stderr,
        )
        return 4
    try:
        commit = resolve_commit(root, commit_reference)
        ensure_no_active_runs(root)
        if not accepted_decision(root, manifest, decision_id):
            raise ValueError(
                f"Release-unit decision {decision_id} is missing or is not accepted"
            )
    except (OSError, ValueError, json.JSONDecodeError, subprocess.TimeoutExpired) as exc:
        print(str(exc), file=sys.stderr)
        return 4

    frozen_at = datetime.now(timezone.utc).isoformat()
    release_units_path = bundle / manifest["artifacts"]["releaseUnits"]
    original_release_units = read_object(release_units_path)
    updated_release_units = json.loads(json.dumps(original_release_units))
    payload = project_release_unit_payload(
        manifest,
        context["artifacts"],
        unit,
        context["closure"],
        context["deferredSnapshot"],
        commit=commit,
        frozen_at=frozen_at,
    )
    release_hash = str(payload["releaseUnitHash"])
    snapshot_path: Path | None = None
    snapshot_created = False
    try:
        if unit.get("baselineReconciliation") is not None:
            from baseline_reconciliation import validate_current_receipt
            validate_current_receipt(root, unit["baselineReconciliation"])
        snapshot_path, snapshot_created = create_release_unit_snapshot(bundle, payload)
        frozen_unit = next(
            item
            for item in updated_release_units["releaseUnits"]
            if isinstance(item, dict) and item.get("id") == release_unit_id
        )
        frozen_unit["status"] = "frozen"
        frozen_unit["closure"] = context["closure"]
        frozen_unit["deferredSnapshot"] = context["deferredSnapshot"]
        frozen_unit["freeze"] = {
            "releaseUnitId": release_unit_id,
            "decisionId": decision_id,
            "commit": commit,
            "frozenAt": frozen_at,
            "releaseUnitHash": release_hash,
            "snapshotPath": release_unit_snapshot_relative(release_hash),
        }
        write_object(release_units_path, updated_release_units)
        frozen = validate_bundle(root, "freeze", release_unit_id)
        if frozen.errors:
            raise ValueError("; ".join(frozen.errors))
        report = {
            "schemaVersion": 1,
            "validatedAt": datetime.now(timezone.utc).isoformat(),
            "mode": "release_unit_freeze",
            "valid": True,
            "errorCount": 0,
            "warningCount": len(frozen.warnings),
            "errors": [],
            "warnings": frozen.warnings,
            "decisionId": decision_id,
            "baselineCommit": commit,
            "releaseUnitId": release_unit_id,
            "releaseUnitHash": release_hash,
            "deferredSnapshot": context["deferredSnapshot"],
        }
        timestamp = frozen_at.replace(":", "-").replace("+", "_")
        report_path = (
            bundle
            / "reports"
            / f"release-unit-{release_unit_id}-validation.{timestamp}.json"
        )
        write_object(report_path, report)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        write_object(release_units_path, original_release_units)
        if snapshot_created and snapshot_path is not None:
            snapshot_path.unlink(missing_ok=True)
            try:
                snapshot_path.parent.rmdir()
            except OSError:
                pass
        print(f"Release-unit seal failed and catalog was restored: {exc}", file=sys.stderr)
        return 5

    print(f"Release unit {release_unit_id} frozen at {release_hash}")
    print(f"Baseline commit: {commit}")
    print(f"Validation report: {report_path}")
    print(f"Release-unit snapshot: {snapshot_path}")
    return 0


def main() -> int:
    args = parse_args()
    root = args.project_root.expanduser().resolve()
    bundle = root / BUNDLE_DIR
    if args.release_unit:
        return seal_release_unit(
            root, args.release_unit, args.decision_id, args.commit
        )
    manifest_path = bundle / "manifest.json"
    try:
        manifest = read_object(manifest_path)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(str(exc), file=sys.stderr)
        return 2

    review = validate_bundle(root, "review")
    if review.errors:
        print("Refusing to seal: review validation failed.", file=sys.stderr)
        for message in review.errors:
            print(f"ERROR: {message}", file=sys.stderr)
        return 3

    try:
        commit = resolve_commit(root, args.commit)
        ensure_no_active_runs(root)
        if not accepted_decision(root, manifest, args.decision_id):
            raise ValueError(
                f"Freeze decision {args.decision_id} is missing or is not accepted"
            )
    except (OSError, ValueError, json.JSONDecodeError, subprocess.TimeoutExpired) as exc:
        print(str(exc), file=sys.stderr)
        return 4

    original_manifest = json.loads(json.dumps(manifest))
    frozen_at = datetime.now(timezone.utc).isoformat()
    manifest["status"] = "frozen"
    manifest["freeze"] = {
        "decisionId": args.decision_id,
        "commit": commit,
        "frozenAt": frozen_at,
        "manifestHash": None,
    }
    try:
        manifest_hash = governance_hash(bundle, manifest)
        manifest["freeze"]["manifestHash"] = manifest_hash
        write_object(manifest_path, manifest)
        frozen = validate_bundle(root, "freeze")
        if frozen.errors:
            raise ValueError("; ".join(frozen.errors))
        snapshot_path = create_freeze_snapshot(bundle, manifest, manifest_hash)
        report = report_payload(root, commit, manifest_hash, args.decision_id)
        timestamp = frozen_at.replace(":", "-").replace("+", "_")
        report_path = bundle / "reports" / f"freeze-validation.{timestamp}.json"
        write_object(report_path, report)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        write_object(manifest_path, original_manifest)
        print(f"Seal failed and manifest was restored: {exc}", file=sys.stderr)
        return 5

    print(f"Governance frozen at {manifest_hash}")
    print(f"Baseline commit: {commit}")
    print(f"Validation report: {report_path}")
    print(f"Freeze snapshot: {snapshot_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
