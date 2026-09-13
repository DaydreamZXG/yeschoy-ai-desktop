#!/usr/bin/env python3
"""Shared integrity checks for governed work-package execution evidence."""

from __future__ import annotations

from fnmatch import fnmatchcase
import hashlib
import json
from pathlib import Path, PurePosixPath
from typing import Any

from governance_common import BUNDLE_DIR, read_object, source_tree_snapshot


REPORT_SCHEMA_VERSION = 3
SUPPORTED_REPORT_SCHEMA_VERSIONS = {2, REPORT_SCHEMA_VERSION}
OWNED_TESTED_STATE_SCHEMA_VERSION = 1
OWNED_TESTED_STATE_POLICY = "allowed-source-files-and-symlinks@1"


def expected_execution_identity(
    manifest_hash: str | None = None,
    *,
    release_unit_id: str | None = None,
    release_unit_hash: str | None = None,
) -> dict[str, str]:
    if release_unit_id is not None or release_unit_hash is not None:
        if not isinstance(release_unit_id, str) or not release_unit_id:
            raise ValueError("releaseUnitId is required for release-unit execution")
        if not isinstance(release_unit_hash, str) or not release_unit_hash.startswith("sha256:"):
            raise ValueError("releaseUnitHash is required for release-unit execution")
        if manifest_hash is not None:
            raise ValueError("Execution cannot mix manifestHash and releaseUnitHash")
        return {
            "releaseUnitId": release_unit_id,
            "releaseUnitHash": release_unit_hash,
        }
    if not isinstance(manifest_hash, str) or not manifest_hash.startswith("sha256:"):
        raise ValueError("manifestHash is required for global execution")
    return {"manifestHash": manifest_hash}


def run_execution_identity(run: dict[str, Any]) -> dict[str, str]:
    has_release = "releaseUnitId" in run or "releaseUnitHash" in run
    has_global = "manifestHash" in run
    if has_release and has_global:
        raise ValueError("Execution run mixes global and release-unit identity")
    if has_release:
        return expected_execution_identity(
            None,
            release_unit_id=run.get("releaseUnitId"),
            release_unit_hash=run.get("releaseUnitHash"),
        )
    return expected_execution_identity(run.get("manifestHash"))


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65_536), b""):
            digest.update(chunk)
    return f"sha256:{digest.hexdigest()}"


def object_sha256(value: Any) -> str:
    payload = json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return f"sha256:{hashlib.sha256(payload).hexdigest()}"


def safe_relative_file(value: Any) -> bool:
    if not isinstance(value, str) or not value or "\\" in value:
        return False
    path = PurePosixPath(value)
    return not path.is_absolute() and not any(part in {"", ".", ".."} for part in path.parts)


def path_matches_pattern(path: str, pattern: str) -> bool:
    if pattern.endswith("/**"):
        base = pattern[:-3].rstrip("/")
        return path == base or path.startswith(f"{base}/")
    return fnmatchcase(path, pattern)


def owned_tested_state(
    package: dict[str, Any], snapshot: dict[str, Any]
) -> dict[str, Any]:
    """Select the complete tested source state owned by one frozen package.

    The source snapshot intentionally contains tracked and non-ignored untracked
    material only.  Regular files and hashed symlink targets are evidence; Git
    directories/submodules, devices, sockets, FIFOs, transient missing markers,
    and the governance execution journal are not source-file evidence.
    """

    allowed = package.get("allowedPaths")
    if not isinstance(allowed, list) or not all(
        isinstance(pattern, str) and pattern for pattern in allowed
    ):
        raise ValueError("package allowedPaths are malformed")
    path_hashes = snapshot.get("pathHashes")
    if not isinstance(path_hashes, dict):
        raise ValueError("source snapshot path hashes are malformed")
    selected: dict[str, str] = {}
    for path, digest in sorted(path_hashes.items()):
        if not isinstance(path, str) or not isinstance(digest, str):
            raise ValueError("source snapshot path hash entry is malformed")
        if path == f"{BUNDLE_DIR}/execution" or path.startswith(
            f"{BUNDLE_DIR}/execution/"
        ):
            continue
        if not (
            digest.startswith("sha256:")
            or digest.startswith("symlink-sha256:")
        ):
            continue
        if any(path_matches_pattern(path, pattern) for pattern in allowed):
            selected[path] = digest
    return {
        "schemaVersion": OWNED_TESTED_STATE_SCHEMA_VERSION,
        "selectionPolicy": OWNED_TESTED_STATE_POLICY,
        "allowedPaths": list(allowed),
        "excludedClasses": [
            "governance_execution_runtime",
            "directories_and_git_submodules",
            "devices_sockets_and_fifos",
            "transient_missing_entries",
        ],
        "pathHashes": selected,
    }


def changed_path_hashes(
    baseline: dict[str, str], current: dict[str, str]
) -> dict[str, str]:
    return {
        path: current.get(path, "deleted")
        for path in sorted(set(baseline) | set(current))
        if baseline.get(path) != current.get(path)
    }


def scope_failures_for_changes(
    package: dict[str, Any], changes: dict[str, str]
) -> list[str]:
    failures: list[str] = []
    allowed = package.get("allowedPaths", [])
    forbidden = package.get("forbiddenPaths", [])
    for path in changes:
        if any(path_matches_pattern(path, pattern) for pattern in forbidden):
            failures.append(f"changed forbidden path: {path}")
        elif not any(path_matches_pattern(path, pattern) for pattern in allowed):
            failures.append(f"changed path outside package ownership: {path}")
    return failures


def run_token(run_id: str) -> str:
    return hashlib.sha256(run_id.encode("utf-8")).hexdigest()[:20]


def package_token(package_id: str) -> str:
    return hashlib.sha256(package_id.encode("utf-8")).hexdigest()[:16]


def scenario_token(scenario_id: str) -> str:
    return hashlib.sha256(scenario_id.encode("utf-8")).hexdigest()[:16]


def report_relative_path(run_id: str, package_id: str) -> str:
    return f"execution/reports/{package_token(package_id)}.{run_token(run_id)}.json"


def baseline_relative_path(run_id: str, package_id: str) -> str:
    return f"execution/baselines/{package_token(package_id)}.{run_token(run_id)}.json"


def tested_snapshot_relative_path(run_id: str, package_id: str) -> str:
    return f"execution/tested-snapshots/{package_token(package_id)}.{run_token(run_id)}.json"


def scenario_evidence_relative_path(
    run_id: str, package_id: str, scenario_id: str
) -> str:
    return (
        f"execution/evidence/{run_token(run_id)}/{package_token(package_id)}/"
        f"{scenario_token(scenario_id)}.json"
    )


def bundle_execution_path(root: Path, relative: str) -> Path:
    if not safe_relative_file(relative) or not relative.startswith("execution/"):
        raise ValueError(f"Unsafe governance execution path: {relative}")
    bundle = (root / BUNDLE_DIR).resolve()
    execution = bundle / "execution"
    if execution.exists() and execution.is_symlink():
        raise ValueError("Governance execution root cannot be a symlink")
    candidate = bundle / relative
    resolved = candidate.resolve(strict=False)
    resolved_execution = execution.resolve(strict=False)
    if resolved_execution not in (resolved, *resolved.parents):
        raise ValueError(f"Governance execution path escapes its root: {relative}")
    current = execution
    for part in PurePosixPath(relative).parts[1:-1]:
        current = current / part
        if current.exists() and current.is_symlink():
            raise ValueError(f"Governance execution path crosses a symlink: {relative}")
    if candidate.exists() and candidate.is_symlink():
        raise ValueError(f"Governance execution file is a symlink: {relative}")
    return candidate


def _read_hashed_snapshot(
    root: Path,
    relative: Any,
    expected_hash: Any,
    expected_tree_hash: Any,
    label: str,
    errors: list[str],
) -> dict[str, Any] | None:
    if not isinstance(relative, str) or not isinstance(expected_hash, str):
        errors.append(f"{label} snapshot locator is missing")
        return None
    try:
        path = bundle_execution_path(root, relative)
    except ValueError as exc:
        errors.append(str(exc))
        return None
    if not path.is_file() or sha256(path) != expected_hash:
        errors.append(f"{label} snapshot is missing or hash-mismatched")
        return None
    try:
        snapshot = read_object(path)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        errors.append(f"{label} snapshot is unreadable: {exc}")
        return None
    if snapshot.get("treeHash") != expected_tree_hash:
        errors.append(f"{label} snapshot tree hash is inconsistent")
    if not isinstance(snapshot.get("pathHashes"), dict):
        errors.append(f"{label} snapshot path hashes are malformed")
        return None
    return snapshot


def current_output_successors(root: Path, run: dict[str, Any]) -> list[dict[str, Any]]:
    """Resolve file ownership handed to later, same-authority implementation runs.

    A completed report remains immutable evidence of its tested state, but an
    explicitly ordered implementation may subsequently change overlapping paths.
    Read-only checks, unrelated packages and merely planned packages never take
    ownership. Failed implementation attempts retain ownership until retried;
    they do not become completed dependencies by doing so.
    """
    registry = read_object(root / BUNDLE_DIR / "execution/work-package-runs.json")
    runs = registry.get("runs")
    if not isinstance(runs, list):
        raise ValueError("execution registry runs are malformed")
    positions = [i for i, item in enumerate(runs)
                 if isinstance(item, dict) and item.get("id") == run.get("id")]
    if len(positions) != 1:
        # A verifier also checks a candidate completed report before journaling it.
        if not positions:
            return []
        raise ValueError("execution registry contains duplicate run identities")
    identity = run_execution_identity(run)
    later = [
        item for item in runs[positions[0] + 1:]
        if isinstance(item, dict)
        and run_execution_identity(item) == identity
        and item.get("status") in {"in_progress", "completed", "failed"}
    ]
    if not later:
        return []
    manifest = read_object(root / BUNDLE_DIR / "manifest.json")
    if "releaseUnitId" in identity:
        # Lazy import: release_units itself uses execution path validation.
        from release_units import load_release_unit_snapshot, release_unit_index

        units = release_unit_index(read_object(
            root / BUNDLE_DIR / manifest["artifacts"]["releaseUnits"]
        ))
        unit = units.get(identity["releaseUnitId"])
        if unit is None or unit.get("status") != "frozen":
            raise ValueError("output successor authority is not frozen")
        snapshot = load_release_unit_snapshot(root, unit)
        if snapshot.get("releaseUnitHash") != identity["releaseUnitHash"]:
            raise ValueError("output successor authority hash mismatch")
        definitions = snapshot["artifacts"]["workPackages"]["packages"]
    else:
        from governance_common import governance_hash

        if (manifest.get("freeze", {}).get("manifestHash") != identity["manifestHash"]
                or governance_hash(root / BUNDLE_DIR, manifest) != identity["manifestHash"]):
            raise ValueError("output successor global authority hash mismatch")
        definitions = read_object(
            root / BUNDLE_DIR / manifest["artifacts"]["workPackages"]
        )["packages"]
    packages = {item["id"]: item for item in definitions}

    def depends_on(candidate_id: str, target: str, seen: set[str]) -> bool:
        if candidate_id in seen or candidate_id not in packages:
            return False
        seen.add(candidate_id)
        dependencies = packages[candidate_id].get("dependsOn", [])
        return target in dependencies or any(
            depends_on(dependency, target, seen) for dependency in dependencies
        )

    successors = []
    for candidate in later:
        candidate_id = candidate.get("packageId")
        package = packages.get(candidate_id)
        if package is None:
            raise ValueError("output successor package is missing from frozen authority")
        original_id = str(run.get("packageId"))
        original = packages.get(original_id)
        if original is None:
            raise ValueError("original package is missing from frozen authority")
        # A new attempt also owns its own mutable outputs. A read-only check
        # records evidence, not a permanent write lock on its implementation
        # dependencies: an explicitly related writer may be dispatched again.
        ordered_writer = (
            candidate_id == original_id
            or depends_on(candidate_id, original_id, set())
            or (original.get("verificationOnly", False)
                and depends_on(original_id, candidate_id, set()))
        )
        if not package.get("verificationOnly", False) and ordered_writer:
            successors.append(package)
    return successors


def validate_completed_run_integrity(
    root: Path,
    run: dict[str, Any],
    package: dict[str, Any],
    manifest_hash: str | None,
    *,
    check_current_outputs: bool,
    release_unit_id: str | None = None,
    release_unit_hash: str | None = None,
) -> list[str]:
    """Recompute all facts needed to trust a completed dependency run."""

    errors: list[str] = []
    run_id = run.get("id")
    package_id = package.get("id")
    if not isinstance(run_id, str) or not isinstance(package_id, str):
        return ["run or package identity is missing"]
    if run.get("status") != "completed":
        errors.append("run is not completed")
    try:
        expected_identity = expected_execution_identity(
            manifest_hash,
            release_unit_id=release_unit_id,
            release_unit_hash=release_unit_hash,
        )
        actual_identity = run_execution_identity(run)
    except ValueError as exc:
        return [str(exc)]
    if run.get("packageId") != package_id or actual_identity != expected_identity:
        errors.append("run identity does not match the frozen package")
    expected_report_relative = report_relative_path(run_id, package_id)
    if run.get("reportPath") != expected_report_relative:
        errors.append("run report path is not verifier-derived")
        return errors
    try:
        report_path = bundle_execution_path(root, expected_report_relative)
    except ValueError as exc:
        errors.append(str(exc))
        return errors
    if not report_path.is_file() or sha256(report_path) != run.get("reportSha256"):
        errors.append("run report is missing or hash-mismatched")
        return errors
    try:
        report = read_object(report_path)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        return [f"run report is unreadable: {exc}"]
    report_schema_version = report.get("schemaVersion")
    if report_schema_version not in SUPPORTED_REPORT_SCHEMA_VERSIONS:
        errors.append("run report schema version is invalid")
    for field, expected in {
        "runId": run_id,
        "packageId": package_id,
        "baselineCommit": run.get("baselineCommit"),
        "baselineTreeHash": run.get("baselineTreeHash"),
        "assignedTo": run.get("assignedTo"),
        "startedAt": run.get("startedAt"),
    }.items():
        if report.get(field) != expected:
            errors.append(f"run report {field} mismatch")
    for field, expected in expected_identity.items():
        if report.get(field) != expected:
            errors.append(f"run report {field} mismatch")
    forbidden_identity_fields = (
        {"manifestHash"} if "releaseUnitHash" in expected_identity else {"releaseUnitId", "releaseUnitHash"}
    )
    for field in forbidden_identity_fields:
        if field in report:
            errors.append(f"run report contains incompatible identity field {field}")
    if report.get("passed") is not True:
        errors.append("run report is not passed")

    expected_baseline_relative = baseline_relative_path(run_id, package_id)
    if run.get("baselineSnapshotPath") != expected_baseline_relative:
        errors.append("baseline snapshot path is not verifier-derived")
    baseline = _read_hashed_snapshot(
        root,
        run.get("baselineSnapshotPath"),
        run.get("baselineSnapshotSha256"),
        run.get("baselineTreeHash"),
        "baseline",
        errors,
    )
    expected_tested_relative = tested_snapshot_relative_path(run_id, package_id)
    if report.get("testedSnapshotPath") != expected_tested_relative:
        errors.append("tested snapshot path is not verifier-derived")
    tested = _read_hashed_snapshot(
        root,
        report.get("testedSnapshotPath"),
        report.get("testedSnapshotSha256"),
        report.get("testedTreeHash"),
        "tested",
        errors,
    )
    if baseline is not None and tested is not None:
        source_diff = changed_path_hashes(
            baseline["pathHashes"], tested["pathHashes"]
        )
        if report.get("sourceDiff") != source_diff:
            errors.append("reported sourceDiff is not the baseline-to-tested diff")
        if report.get("ownedResultHashes") != source_diff:
            errors.append("ownedResultHashes is not the complete source diff")
        recomputed_scope = scope_failures_for_changes(package, source_diff)
        if report.get("sourceScopeFailures") != recomputed_scope:
            errors.append("source scope failures do not match the recomputed diff")
        if recomputed_scope:
            errors.append("completed run changed paths outside package ownership")
        if package.get("verificationOnly", False) and source_diff:
            errors.append("verification-only work package changed project source")
        elif not package.get("verificationOnly", False) and not source_diff:
            errors.append("non-verification work package produced no source change")
        try:
            expected_owned_state = owned_tested_state(package, tested)
        except ValueError as exc:
            errors.append(str(exc))
            expected_owned_state = None
        if report_schema_version == REPORT_SCHEMA_VERSION:
            if report.get("testedOwnedState") != expected_owned_state:
                errors.append(
                    "testedOwnedState is not the complete allowedPaths tested state"
                )
            if (
                expected_owned_state is None
                or report.get("testedOwnedStateSha256")
                != object_sha256(expected_owned_state)
            ):
                errors.append("testedOwnedStateSha256 is invalid")
        if check_current_outputs:
            try:
                successors = current_output_successors(root, run)
                current_owned_state = owned_tested_state(
                    package, source_tree_snapshot(root)
                )
            except (KeyError, OSError, ValueError) as exc:
                errors.append(str(exc))
                current_owned_state = None
            if expected_owned_state is not None and current_owned_state is not None:
                drift = changed_path_hashes(
                    expected_owned_state["pathHashes"],
                    current_owned_state["pathHashes"],
                )
                for path in drift:
                    if any(
                        any(path_matches_pattern(path, pattern)
                            for pattern in successor.get("allowedPaths", []))
                        and not any(path_matches_pattern(path, pattern)
                                    for pattern in successor.get("forbiddenPaths", []))
                        for successor in successors
                    ):
                        continue
                    errors.append(
                        f"completed tested owned state changed after verification: {path}"
                    )

    declared_commands = package.get("acceptanceCommands", [])
    command_results = report.get("commands")
    if not isinstance(declared_commands, list) or not isinstance(command_results, list):
        errors.append("declared commands or command results are malformed")
        command_results = []
    declared_by_id = {
        item.get("id"): item
        for item in declared_commands
        if isinstance(item, dict) and isinstance(item.get("id"), str)
    }
    results_by_id: dict[str, dict[str, Any]] = {}
    for result in command_results:
        if not isinstance(result, dict) or not isinstance(result.get("id"), str):
            errors.append("command result is malformed")
            continue
        if result["id"] in results_by_id:
            errors.append(f"duplicate command result: {result['id']}")
        results_by_id[result["id"]] = result
    if set(results_by_id) != set(declared_by_id):
        errors.append("command result set does not equal the frozen command set")
    runner_proofs: dict[str, dict[str, Any]] = {}
    for command_id, command in declared_by_id.items():
        result = results_by_id.get(command_id)
        if result is None:
            continue
        if result.get("declaredCommand") != command:
            errors.append(f"command result declaration mismatch: {command_id}")
            continue
        if result.get("passed") is not True or result.get("actualExitCode") != 0 or result.get("timedOut") is not False:
            errors.append(f"command did not pass exactly: {command_id}")
            continue
        if command.get("kind") == "scenario_test":
            from runner_adapters import verify_archived_scenario_result

            proof = verify_archived_scenario_result(command, result)
            if proof is None or result.get("runnerProof") != proof:
                errors.append(f"scenario runner proof is invalid: {command_id}")
            else:
                runner_proofs[command_id] = proof

    declared_scenarios = package.get("requiredAcceptanceScenarios", [])
    scenario_results = report.get("scenarioEvidence")
    if not isinstance(declared_scenarios, list) or not isinstance(scenario_results, list):
        errors.append("declared scenarios or scenario evidence are malformed")
        scenario_results = []
    scenario_by_id: dict[str, dict[str, Any]] = {}
    for item in scenario_results:
        if not isinstance(item, dict) or not isinstance(item.get("scenarioId"), str):
            errors.append("scenario evidence locator is malformed")
            continue
        if item["scenarioId"] in scenario_by_id:
            errors.append(f"duplicate scenario evidence: {item['scenarioId']}")
        scenario_by_id[item["scenarioId"]] = item
    declared_scenario_by_id = {
        item.get("id"): item
        for item in declared_scenarios
        if isinstance(item, dict) and isinstance(item.get("id"), str)
    }
    if set(scenario_by_id) != set(declared_scenario_by_id):
        errors.append("scenario evidence set does not equal the frozen scenario set")
    for scenario_id, scenario in declared_scenario_by_id.items():
        locator = scenario_by_id.get(scenario_id)
        if locator is None:
            continue
        expected_relative = scenario_evidence_relative_path(run_id, package_id, scenario_id)
        if locator.get("path") != expected_relative:
            errors.append(f"scenario evidence path is not verifier-derived: {scenario_id}")
            continue
        try:
            path = bundle_execution_path(root, expected_relative)
        except ValueError as exc:
            errors.append(str(exc))
            continue
        if not path.is_file() or sha256(path) != locator.get("sha256"):
            errors.append(f"scenario evidence is missing or hash-mismatched: {scenario_id}")
            continue
        try:
            payload = read_object(path)
        except (OSError, ValueError, json.JSONDecodeError) as exc:
            errors.append(f"scenario evidence is unreadable ({scenario_id}): {exc}")
            continue
        command_id = scenario.get("acceptanceCommandId")
        command_result = results_by_id.get(command_id)
        expected_values = {
            "scenarioId": scenario_id,
            "assertion": scenario.get("assertion"),
            "testSelector": scenario.get("testSelector"),
            "acceptanceCommandId": command_id,
            "runId": run_id,
            "packageId": package_id,
            "evidenceOwner": "govern-product-build/verifier",
            "passed": True,
            "runnerProof": runner_proofs.get(command_id),
            "commandResultHash": object_sha256(command_result),
        }
        expected_values.update(expected_identity)
        for field, expected in expected_values.items():
            if payload.get(field) != expected:
                errors.append(f"scenario evidence {field} mismatch: {scenario_id}")
    return errors
