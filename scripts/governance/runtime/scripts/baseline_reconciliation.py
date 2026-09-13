"""Explicit, content-addressed handoff of drifted predecessor outputs.

A receipt acknowledges the exact current source state, not successful tests.
Only a review unit with an accepted, explicit reconciliation decision may use
one. Historical reports are always reverified. Freeze/dispatch still require
the ordinary gates and new runs still need fresh executable evidence.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

from execution_integrity import (
    bundle_execution_path, changed_path_hashes, object_sha256, owned_tested_state,
    path_matches_pattern, sha256, validate_completed_run_integrity,
)
from governance_common import BUNDLE_DIR, read_object, source_tree_snapshot, write_object


def decision_for_reconciliation(unit: dict, decisions: list[dict]) -> dict:
    matches = [d for d in decisions if d.get("id") == unit.get("decisionRef")]
    if len(matches) != 1 or matches[0].get("status") != "accepted" or (
        matches[0].get("approvesBaselineReconciliation") is not True
    ):
        raise ValueError("Baseline reconciliation requires an explicit accepted decision")
    return matches[0]


def capture_state(root: Path, unit: dict, units: dict, decisions: list[dict]) -> dict:
    from release_units import load_release_unit_snapshot, frozen_release_unit_superseders

    decision_for_reconciliation(unit, decisions)
    manifest = read_object(root / BUNDLE_DIR / "manifest.json")
    definitions = read_object(root / BUNDLE_DIR / manifest["artifacts"]["workPackages"])["packages"]
    root_ids = unit.get("roots", {}).get("workPackages", [])
    successors = [p for p in definitions if p.get("id") in root_ids and p.get("status") == "planned"]
    if not root_ids or {p["id"] for p in successors} != set(root_ids):
        raise ValueError("Baseline reconciliation requires planned root-package ownership")
    predecessors = unit.get("supersedesReleaseUnits", [])
    if not predecessors or not set(predecessors).issubset(unit.get("dependsOnReleaseUnits", [])):
        raise ValueError("Baseline reconciliation requires explicit predecessor dependencies and supersession")
    registry = read_object(root / BUNDLE_DIR / "execution/work-package-runs.json")
    runs = registry.get("runs")
    if not isinstance(runs, list) or any(not isinstance(r, dict) for r in runs):
        raise ValueError("Baseline reconciliation requires a valid run registry")
    if any(r.get("status") == "in_progress" for r in runs):
        raise ValueError("Baseline reconciliation cannot replace an active run")
    current = source_tree_snapshot(root)
    captured: list[dict[str, Any]] = []
    for predecessor_id in sorted(predecessors):
        predecessor = units.get(predecessor_id)
        if not predecessor or predecessor.get("status") != "frozen":
            raise ValueError("Baseline reconciliation predecessor must be frozen")
        if frozen_release_unit_superseders(units, predecessor_id):
            raise ValueError("Baseline reconciliation predecessor is already superseded")
        snapshot = load_release_unit_snapshot(root, predecessor)
        packages = {p["id"]: p for p in snapshot["artifacts"]["workPackages"]["packages"]}
        latest = {}
        for run in runs:
            if (run.get("releaseUnitId") == predecessor_id
                    and run.get("releaseUnitHash") == snapshot["releaseUnitHash"]
                    and run.get("status") == "completed"):
                latest[run["packageId"]] = run
        if not latest:
            raise ValueError("Baseline reconciliation requires a completed predecessor proof")
        for package_id, run in sorted(latest.items()):
            package = packages.get(package_id)
            if package is None:
                raise ValueError("Baseline reconciliation predecessor package is missing")
            errors = validate_completed_run_integrity(
                root, run, package, None, check_current_outputs=False,
                release_unit_id=predecessor_id, release_unit_hash=snapshot["releaseUnitHash"],
            )
            if errors:
                raise ValueError("Baseline reconciliation cannot excuse invalid historical proof: " + "; ".join(errors))
            report = read_object(bundle_execution_path(root, run["reportPath"]))
            tested = read_object(bundle_execution_path(root, report["testedSnapshotPath"]))
            before = owned_tested_state(package, tested)
            after = owned_tested_state(package, current)
            changed = changed_path_hashes(before["pathHashes"], after["pathHashes"])
            for path in changed:
                if not any(
                    any(path_matches_pattern(path, pattern) for pattern in p.get("allowedPaths", []))
                    and not any(path_matches_pattern(path, pattern) for pattern in p.get("forbiddenPaths", []))
                    for p in successors
                ):
                    raise ValueError(f"Baseline reconciliation cannot transfer unowned drift: {path}")
            captured.append({
                "runId": run["id"], "releaseUnitId": predecessor_id,
                "releaseUnitHash": snapshot["releaseUnitHash"], "packageId": package_id,
                "reportPath": run["reportPath"], "reportSha256": run["reportSha256"],
                "previousOwnedStateSha256": object_sha256(before),
                "currentOwnedState": after,
                "changedPaths": changed,
            })
    return {
        "schemaVersion": 1, "purpose": "acknowledged_baseline_not_test_evidence",
        "releaseUnitId": unit["id"], "decisionId": unit["decisionRef"],
        "predecessorReleaseUnits": sorted(predecessors), "runs": captured,
    }


def read_receipt(root: Path, binding: dict) -> dict:
    if not isinstance(binding, dict) or set(binding) != {"receiptPath", "receiptSha256"}:
        raise ValueError("Baseline reconciliation binding is malformed")
    digest = binding.get("receiptSha256")
    if not isinstance(digest, str) or len(digest) != 71 or not digest.startswith("sha256:"):
        raise ValueError("Baseline reconciliation hash is invalid")
    expected = f"execution/baseline-reconciliations/{digest[7:]}.json"
    if binding.get("receiptPath") != expected:
        raise ValueError("Baseline reconciliation path must be content-addressed")
    path = bundle_execution_path(root, expected)
    if not path.is_file() or sha256(path) != digest:
        raise ValueError("Baseline reconciliation receipt is missing or changed")
    return read_object(path)


def reconciled_run_ids(root: Path, unit: dict, units: dict, decisions: list[dict]) -> set[str]:
    binding = unit.get("baselineReconciliation")
    if binding is None:
        return set()
    if unit.get("status") != "review":
        raise ValueError("Baseline reconciliation applies only to a review transition")
    receipt = read_receipt(root, binding)
    expected = capture_state(root, unit, units, decisions)
    if receipt != expected:
        raise ValueError("Baseline reconciliation is stale or does not match this unit's exact predecessor state")
    return {r["runId"] for r in receipt["runs"]}


def validate_archived_receipt(root: Path, payload: dict) -> None:
    """A frozen receipt keeps referenced historical runs mandatory forever."""
    from release_units import load_release_unit_snapshot, release_unit_index
    receipt = read_receipt(root, payload["baselineReconciliation"])
    if (receipt.get("releaseUnitId") != payload.get("releaseUnitId")
            or receipt.get("decisionId") != payload.get("decisionId")
            or receipt.get("predecessorReleaseUnits") != sorted(payload.get("supersedesReleaseUnits", []))):
        raise ValueError("Archived baseline reconciliation identity mismatch")
    decision_for_reconciliation(
        {"decisionRef": payload["decisionId"]},
        payload["artifacts"]["decisionRegister"]["decisions"],
    )
    manifest = read_object(root / BUNDLE_DIR / "manifest.json")
    units = release_unit_index(read_object(root / BUNDLE_DIR / manifest["artifacts"]["releaseUnits"]))
    runs = read_object(root / BUNDLE_DIR / "execution/work-package-runs.json")["runs"]
    for captured in receipt["runs"]:
        matches = [r for r in runs if r.get("id") == captured["runId"]]
        if len(matches) != 1 or any(matches[0].get(k) != captured[k] for k in (
            "reportPath", "reportSha256", "releaseUnitId", "releaseUnitHash", "packageId"
        )):
            raise ValueError("Archived baseline reconciliation lost or changed its referenced run")
        predecessor = load_release_unit_snapshot(root, units[captured["releaseUnitId"]])
        if predecessor["releaseUnitHash"] != captured["releaseUnitHash"]:
            raise ValueError("Archived baseline reconciliation predecessor hash mismatch")
        package = next(p for p in predecessor["artifacts"]["workPackages"]["packages"] if p["id"] == captured["packageId"])
        errors = validate_completed_run_integrity(
            root, matches[0], package, None, check_current_outputs=False,
            release_unit_id=captured["releaseUnitId"], release_unit_hash=captured["releaseUnitHash"],
        )
        if errors:
            raise ValueError("Archived baseline reconciliation has invalid historical proof: " + "; ".join(errors))


def validate_current_receipt(root: Path, binding: dict, current: dict | None = None) -> None:
    receipt = read_receipt(root, binding)
    snapshot = current if current is not None else source_tree_snapshot(root)
    for captured in receipt["runs"]:
        acknowledged = captured["currentOwnedState"]
        now = owned_tested_state({"allowedPaths": acknowledged["allowedPaths"]}, snapshot)
        if now != acknowledged:
            raise ValueError("Source changed after baseline reconciliation; capture a new acknowledged baseline")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project-root", type=Path, required=True)
    parser.add_argument("--release-unit", required=True)
    args = parser.parse_args()
    from release_units import release_unit_index
    root = args.project_root.resolve()
    manifest = read_object(root / BUNDLE_DIR / "manifest.json")
    units = release_unit_index(read_object(root / BUNDLE_DIR / manifest["artifacts"]["releaseUnits"]))
    decisions = read_object(root / BUNDLE_DIR / manifest["artifacts"]["decisionRegister"])["decisions"]
    unit = units.get(args.release_unit)
    if not unit or unit.get("status") != "review":
        raise ValueError("Select an existing review unit")
    receipt = capture_state(root, unit, units, decisions)
    # Use the same serializer as write_object so the filename hashes exact bytes.
    rendered = json.dumps(receipt, ensure_ascii=False, indent=2) + "\n"
    import hashlib
    digest = "sha256:" + hashlib.sha256(rendered.encode()).hexdigest()
    relative = f"execution/baseline-reconciliations/{digest[7:]}.json"
    path = bundle_execution_path(root, relative)
    if path.exists():
        if sha256(path) != digest:
            raise ValueError("Baseline reconciliation receipt collision")
    else:
        write_object(path, receipt)
    print(json.dumps({"receiptPath": relative, "receiptSha256": digest}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
