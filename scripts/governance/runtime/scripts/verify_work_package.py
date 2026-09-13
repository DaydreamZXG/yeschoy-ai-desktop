#!/usr/bin/env python3
"""Execute frozen acceptance adapters and record recomputable package evidence."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
from typing import Any

from execution_integrity import (
    REPORT_SCHEMA_VERSION,
    bundle_execution_path,
    changed_path_hashes,
    expected_execution_identity,
    object_sha256,
    owned_tested_state,
    report_relative_path,
    scenario_evidence_relative_path,
    scope_failures_for_changes,
    sha256,
    tested_snapshot_relative_path,
    run_execution_identity,
    validate_completed_run_integrity,
)
from governance_common import BUNDLE_DIR, read_object, source_tree_snapshot, write_object
from release_units import (
    load_release_unit_snapshot,
    release_unit_index,
    snapshot_package,
)
from runner_adapters import execute_scenario_command, verify_scenario_result
from validate_governance import safe_relative_file, validate_bundle


RUNS_PATH = Path("execution/work-package-runs.json")
MAX_CAPTURE_CHARS = 100_000


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project-root", required=True, type=Path)
    parser.add_argument("--run-id", required=True)
    return parser.parse_args()


def load_package(
    root: Path, package_id: str, identity: dict[str, str]
) -> dict[str, Any]:
    bundle = root / BUNDLE_DIR
    manifest = read_object(bundle / "manifest.json")
    release_unit_id = identity.get("releaseUnitId")
    if release_unit_id is not None:
        release_path = manifest.get("artifacts", {}).get("releaseUnits")
        if not isinstance(release_path, str):
            raise ValueError("manifest.artifacts.releaseUnits is missing")
        units = release_unit_index(read_object(bundle / release_path))
        unit = units.get(release_unit_id)
        if unit is None:
            raise ValueError(f"Unknown release unit: {release_unit_id}")
        snapshot = load_release_unit_snapshot(root, unit)
        if snapshot.get("releaseUnitHash") != identity.get("releaseUnitHash"):
            raise ValueError("Run releaseUnitHash does not match its frozen snapshot")
        return snapshot_package(snapshot, package_id)
    data = read_object(bundle / manifest["artifacts"]["workPackages"])
    for item in data.get("packages", []):
        if isinstance(item, dict) and item.get("id") == package_id:
            return item
    raise ValueError(f"Unknown work package: {package_id}")


def execution_environment(
    root: Path,
    command: dict[str, Any],
    run_id: str,
    identity: dict[str, str],
    scenario: dict[str, Any] | None,
) -> dict[str, str]:
    environment = os.environ.copy()
    environment.update(
        {
            "CI": "1",
            "PYTHONDONTWRITEBYTECODE": "1",
            "GOVERNANCE_RUN_ID": run_id,
            "GOVERNANCE_AUTHORITY_HASH": identity.get(
                "releaseUnitHash", identity.get("manifestHash", "")
            ),
            "GOVERNANCE_ACCEPTANCE_COMMAND_ID": str(command["id"]),
            "GOVERNANCE_PROJECT_ROOT": str(root),
        }
    )
    if "manifestHash" in identity:
        environment["GOVERNANCE_MANIFEST_HASH"] = identity["manifestHash"]
    else:
        environment["GOVERNANCE_RELEASE_UNIT_ID"] = identity["releaseUnitId"]
        environment["GOVERNANCE_RELEASE_UNIT_HASH"] = identity["releaseUnitHash"]
    if scenario is not None:
        environment["GOVERNANCE_SCENARIO_ID"] = str(scenario["id"])
    return environment


def readonly_check_invocation(root: Path, argv: list[str]) -> tuple[list[str], dict[str, Any]]:
    """Run supplementary checks with project writes denied; never degrade silently."""

    if platform.system() != "Darwin" or not Path("/usr/bin/sandbox-exec").is_file():
        raise RuntimeError("No registered read-only OS isolation adapter is available")
    escaped = str(root).replace("\\", "\\\\").replace('"', '\\"')
    profile = (
        '(version 1) (allow default) '
        f'(deny file-write* (subpath "{escaped}"))'
    )
    return (
        ["/usr/bin/sandbox-exec", "-p", profile, *argv],
        {
            "adapter": "darwin-sandbox-exec-readonly@1",
            "policy": "deny file-write* under the resolved project root",
            "projectRoot": str(root),
        },
    )


def execute_check_command(
    root: Path,
    command: dict[str, Any],
    environment: dict[str, str],
) -> dict[str, Any]:
    cwd_value = command.get("cwd", ".")
    if not safe_relative_file(cwd_value):
        raise ValueError(f"Unsafe acceptance command cwd: {cwd_value}")
    cwd = (root / cwd_value).resolve()
    if root not in (cwd, *cwd.parents) or not cwd.is_dir():
        raise ValueError(f"Acceptance command cwd is outside the project: {cwd_value}")
    declared_argv = command.get("argv")
    if not isinstance(declared_argv, list) or not all(isinstance(item, str) for item in declared_argv):
        raise ValueError(f"Malformed check command argv: {command.get('id')}")
    invocation, isolation = readonly_check_invocation(root, declared_argv)
    started_at = datetime.now(timezone.utc).isoformat()
    try:
        result = subprocess.run(
            invocation,
            cwd=cwd,
            env=environment,
            check=False,
            capture_output=True,
            text=True,
            timeout=command["timeoutSeconds"],
        )
        actual_exit = result.returncode
        stdout = result.stdout[-MAX_CAPTURE_CHARS:]
        stderr = result.stderr[-MAX_CAPTURE_CHARS:]
        timed_out = False
    except subprocess.TimeoutExpired as exc:
        actual_exit = None
        stdout = str(exc.stdout or "")[-MAX_CAPTURE_CHARS:]
        stderr = str(exc.stderr or "")[-MAX_CAPTURE_CHARS:]
        timed_out = True
    return {
        "id": command["id"],
        "kind": "check",
        "declaredCommand": command,
        "effectiveInvocation": invocation,
        "isolation": isolation,
        "startedAt": started_at,
        "finishedAt": datetime.now(timezone.utc).isoformat(),
        "actualExitCode": actual_exit,
        "timedOut": timed_out,
        "passed": not timed_out and actual_exit == 0,
        "stdout": stdout,
        "stderr": stderr,
    }


def failed_command_result(
    command: dict[str, Any], message: str, scenario_id: str | None
) -> dict[str, Any]:
    now = datetime.now(timezone.utc).isoformat()
    return {
        "id": command.get("id"),
        "kind": command.get("kind"),
        "declaredCommand": command,
        "startedAt": now,
        "finishedAt": now,
        "actualExitCode": None,
        "timedOut": False,
        "passed": False,
        "stdout": "",
        "stderr": message,
        "scenarioId": scenario_id,
        "runnerProof": None,
    }


def write_scenario_evidence(
    root: Path,
    package: dict[str, Any],
    scenario: dict[str, Any],
    command_result: dict[str, Any] | None,
    run_id: str,
    identity: dict[str, str],
) -> tuple[dict[str, Any], str | None]:
    command = next(
        (
            item
            for item in package.get("acceptanceCommands", [])
            if isinstance(item, dict)
            and item.get("id") == scenario.get("acceptanceCommandId")
        ),
        None,
    )
    proof = (
        verify_scenario_result(command, command_result)
        if isinstance(command, dict) and isinstance(command_result, dict)
        else None
    )
    passed = proof is not None and command_result.get("passed") is True
    relative = scenario_evidence_relative_path(
        run_id, str(package["id"]), str(scenario["id"])
    )
    path = bundle_execution_path(root, relative)
    payload = {
        "scenarioId": scenario["id"],
        "assertion": scenario["assertion"],
        "testSelector": scenario["testSelector"],
        "acceptanceCommandId": scenario["acceptanceCommandId"],
        "runId": run_id,
        "packageId": package["id"],
        "evidenceOwner": "govern-product-build/verifier",
        "passed": passed,
        "runnerProof": proof,
        "commandResultHash": object_sha256(command_result),
    }
    payload.update(identity)
    write_object(path, payload)
    locator = {
        "scenarioId": scenario["id"],
        "path": relative,
        "sha256": sha256(path),
    }
    failure = None if passed else f"{scenario['id']}: registered runner proof is invalid"
    return locator, failure


def main() -> int:
    args = parse_args()
    root = args.project_root.expanduser().resolve()
    runs_path = root / BUNDLE_DIR / RUNS_PATH
    try:
        registry = read_object(runs_path)
        runs = registry.get("runs")
        if not isinstance(runs, list):
            raise ValueError("Execution registry runs must be an array")
        run = next(
            (item for item in runs if isinstance(item, dict) and item.get("id") == args.run_id),
            None,
        )
        if run is None:
            raise ValueError(f"Unknown run: {args.run_id}")
        if run.get("status") != "in_progress":
            raise ValueError(f"Run {args.run_id} is not in progress")
        identity = run_execution_identity(run)
        validation = validate_bundle(
            root, "freeze", identity.get("releaseUnitId")
        )
        if validation.errors:
            print("Cannot verify against an invalid or changed freeze", file=sys.stderr)
            return 3
        manifest = read_object(root / BUNDLE_DIR / "manifest.json")
        if "manifestHash" in identity:
            current_identity = expected_execution_identity(
                str(manifest["freeze"]["manifestHash"])
            )
            if identity != current_identity:
                raise ValueError("Run manifest hash does not equal the current frozen hash")
        package = load_package(root, str(run.get("packageId")), identity)

        baseline_relative = run.get("baselineSnapshotPath")
        baseline_expected_hash = run.get("baselineSnapshotSha256")
        if not isinstance(baseline_relative, str):
            raise ValueError("Run lacks a baseline source-tree snapshot")
        baseline_path = bundle_execution_path(root, baseline_relative)
        if not baseline_path.is_file() or sha256(baseline_path) != baseline_expected_hash:
            raise ValueError("Run baseline source-tree snapshot is missing or hash-mismatched")
        baseline_snapshot = read_object(baseline_path)
        if baseline_snapshot.get("treeHash") != run.get("baselineTreeHash"):
            raise ValueError("Run baseline source-tree hash is inconsistent")
        baseline_hashes = baseline_snapshot.get("pathHashes")
        if not isinstance(baseline_hashes, dict):
            raise ValueError("Run baseline source-tree path hashes are malformed")

        tested_snapshot = source_tree_snapshot(root)
        tested_hashes = tested_snapshot["pathHashes"]
        source_diff = changed_path_hashes(baseline_hashes, tested_hashes)
        source_scope_failures = scope_failures_for_changes(package, source_diff)
        tested_owned_state = owned_tested_state(package, tested_snapshot)
        integrity_failures: list[str] = []
        if package.get("verificationOnly", False) and source_diff:
            integrity_failures.append(
                "verification-only work package changed project source"
            )
        elif not package.get("verificationOnly", False) and not source_diff:
            integrity_failures.append(
                "non-verification work package produced no source change"
            )

        scenarios_by_command = {
            scenario["acceptanceCommandId"]: scenario
            for scenario in package.get("requiredAcceptanceScenarios", [])
            if isinstance(scenario, dict)
            and isinstance(scenario.get("acceptanceCommandId"), str)
        }
        command_results: list[dict[str, Any]] = []
        if not source_scope_failures and not integrity_failures:
            for command in package["acceptanceCommands"]:
                scenario = scenarios_by_command.get(command.get("id"))
                environment = execution_environment(
                    root, command, args.run_id, identity, scenario
                )
                try:
                    if command.get("kind") == "scenario_test":
                        result = execute_scenario_command(root, command, environment)
                        process = result.get("process")
                        result["id"] = command["id"]
                        result["kind"] = "scenario_test"
                        result["actualExitCode"] = (
                            process.get("actualExitCode")
                            if isinstance(process, dict)
                            else None
                        )
                        result["timedOut"] = (
                            process.get("timedOut")
                            if isinstance(process, dict)
                            else False
                        )
                        result["scenarioId"] = scenario.get("id") if scenario else None
                    elif command.get("kind") == "check":
                        result = execute_check_command(root, command, environment)
                    else:
                        raise ValueError(f"Unsupported acceptance command kind: {command.get('kind')}")
                except (OSError, RuntimeError, TypeError, ValueError, subprocess.SubprocessError) as exc:
                    result = failed_command_result(
                        command, str(exc), scenario.get("id") if scenario else None
                    )
                command_results.append(result)

        post_command_snapshot = source_tree_snapshot(root)
        if post_command_snapshot["treeHash"] != tested_snapshot["treeHash"]:
            source_scope_failures.append(
                "source tree changed during acceptance execution"
            )

        tested_relative = tested_snapshot_relative_path(args.run_id, str(package["id"]))
        tested_path = bundle_execution_path(root, tested_relative)
        write_object(tested_path, tested_snapshot)

        results_by_id = {
            item["id"]: item
            for item in command_results
            if isinstance(item, dict) and isinstance(item.get("id"), str)
        }
        scenario_locators: list[dict[str, Any]] = []
        scenario_failures: list[str] = []
        for scenario in package.get("requiredAcceptanceScenarios", []):
            if not isinstance(scenario, dict):
                scenario_failures.append("malformed scenario declaration")
                continue
            locator, failure = write_scenario_evidence(
                root,
                package,
                scenario,
                results_by_id.get(scenario.get("acceptanceCommandId")),
                args.run_id,
                identity,
            )
            scenario_locators.append(locator)
            if failure:
                scenario_failures.append(failure)

        passed = (
            len(command_results) == len(package.get("acceptanceCommands", []))
            and bool(command_results)
            and all(item.get("passed") is True for item in command_results)
            and not scenario_failures
            and not source_scope_failures
            and not integrity_failures
        )
        finished_at = datetime.now(timezone.utc).isoformat()
        report = {
            "schemaVersion": REPORT_SCHEMA_VERSION,
            "runId": args.run_id,
            "packageId": run["packageId"],
            "baselineCommit": run["baselineCommit"],
            "baselineTreeHash": run["baselineTreeHash"],
            "testedTreeHash": tested_snapshot["treeHash"],
            "testedSnapshotPath": tested_relative,
            "testedSnapshotSha256": sha256(tested_path),
            "sourceDiff": source_diff,
            "ownedResultHashes": source_diff,
            "testedOwnedState": tested_owned_state,
            "testedOwnedStateSha256": object_sha256(tested_owned_state),
            "sourceScopeFailures": source_scope_failures,
            "integrityFailures": integrity_failures,
            "assignedTo": run["assignedTo"],
            "startedAt": run["startedAt"],
            "finishedAt": finished_at,
            "passed": passed,
            "commands": command_results,
            "scenarioEvidence": scenario_locators,
            "scenarioEvidenceFailures": scenario_failures,
        }
        report.update(identity)
        report_relative = report_relative_path(args.run_id, str(run["packageId"]))
        report_path = bundle_execution_path(root, report_relative)
        write_object(report_path, report)

        run["status"] = "completed" if passed else "failed"
        run["finishedAt"] = finished_at
        run["reportPath"] = report_relative
        run["reportSha256"] = sha256(report_path)
        if passed:
            recomputed_failures = validate_completed_run_integrity(
                root,
                run,
                package,
                identity.get("manifestHash"),
                check_current_outputs=True,
                release_unit_id=identity.get("releaseUnitId"),
                release_unit_hash=identity.get("releaseUnitHash"),
            )
            if recomputed_failures:
                passed = False
                report["passed"] = False
                report["integrityFailures"] = recomputed_failures
                write_object(report_path, report)
                run["status"] = "failed"
                run["reportSha256"] = sha256(report_path)
        write_object(runs_path, registry)
    except (KeyError, OSError, TypeError, ValueError, json.JSONDecodeError) as exc:
        print(str(exc), file=sys.stderr)
        return 4
    print(f"{run['status']}: {report_path}")
    return 0 if passed else 5


if __name__ == "__main__":
    raise SystemExit(main())
