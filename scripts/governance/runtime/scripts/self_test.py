#!/usr/bin/env python3
"""Exercise governance initialization, blocking gates, sealing, and tamper detection."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
import tempfile
from typing import Any

from execution_integrity import (
    bundle_execution_path,
    report_relative_path,
    scenario_evidence_relative_path,
    sha256,
)
from dispatch_work_package import completed_run_is_verified
from governance_common import BUNDLE_DIR, read_object, write_object
from init_governance_bundle import build_files
from release_units import compute_release_unit_closure, project_release_unit_payload
from runner_adapters import (
    execute_scenario_command,
    verify_archived_scenario_result,
    verify_scenario_result,
)
from validate_governance import POLICY_PATH, validate_bundle


TRACKS = ["xingce", "essay"]


def assert_valid(
    root: Path, mode: str, *, release_unit_id: str | None = None
) -> None:
    result = validate_bundle(root, mode, release_unit_id)
    if result.errors:
        raise AssertionError(f"Expected valid {mode}: {result.errors}")


def assert_error(
    root: Path,
    mode: str,
    fragment: str,
    *,
    release_unit_id: str | None = None,
) -> None:
    result = validate_bundle(root, mode, release_unit_id)
    if not any(fragment in item for item in result.errors):
        raise AssertionError(
            f"Expected {mode} error containing {fragment!r}; got {result.errors}"
        )


def initialize_git(root: Path) -> None:
    commands = (
        ["git", "init", "-q"],
        ["git", "config", "user.email", "governance-test@example.invalid"],
        ["git", "config", "user.name", "governance-test"],
        ["git", "commit", "--allow-empty", "-qm", "baseline"],
    )
    for command in commands:
        subprocess.run(command, cwd=root, check=True, capture_output=True, text=True)


def exercise_pytest_root_rebasing(root: Path) -> None:
    """Prove nested pytest rootdirs cannot invalidate an exact selector."""

    backend = root / "runner-root-rebase" / "backend"
    test_path = backend / "tests" / "test_probe.py"
    test_path.parent.mkdir(parents=True, exist_ok=True)
    (backend / "pyproject.toml").write_text(
        '[tool.pytest.ini_options]\ntestpaths = ["tests"]\n', encoding="utf-8"
    )
    test_path.write_text("def test_probe():\n    assert True\n", encoding="utf-8")
    command = {
        "id": "pytest-root-rebase",
        "kind": "scenario_test",
        "adapter": "pytest-json@1",
        "selector": (
            "runner-root-rebase/backend/tests/test_probe.py::test_probe"
        ),
        "cwd": ".",
        "timeoutSeconds": 30,
    }
    result = execute_scenario_command(root, command, {})
    if result.get("passed") is not True:
        raise AssertionError(
            "A selector rebased by a nested pytest rootdir lost its runner proof: "
            f"{result}"
        )
    report = result.get("machineReport", {})
    if report.get("pytestRootPath") != str(backend.resolve()):
        raise AssertionError("Pytest rootdir was not bound into the machine report")
    if report.get("collectedNodeIds") != ["tests/test_probe.py::test_probe"]:
        raise AssertionError("The rootdir regression fixture did not rebase its node ID")
    test_path.unlink()
    if verify_scenario_result(command, result) is not None:
        raise AssertionError("Live runner proof ignored a missing current selector")
    if verify_archived_scenario_result(command, result) is None:
        raise AssertionError(
            "Archived runner proof was incorrectly coupled to current selector files"
        )


def write_fixture(root: Path) -> None:
    policy = read_object(POLICY_PATH)
    files = build_files("Governance Self Test", TRACKS, policy)
    bundle = root / BUNDLE_DIR
    bundle.mkdir()
    (bundle / "reports").mkdir()
    (bundle / "execution").mkdir()
    (bundle / "execution" / "reports").mkdir()
    write_object(bundle / "execution" / "work-package-runs.json", {"schemaVersion": 1, "runs": []})
    for name, value in files.items():
        write_object(bundle / name, value)

    observed_at = "2026-01-01T00:00:00+00:00"
    evidence = {
        "schemaVersion": 1,
        "entries": [
            {
                "id": "E-user-scope",
                "claim": "The test product has two declared tracks.",
                "class": "user_confirmed",
                "locator": "conversation:self-test:scope",
                "observedAt": observed_at,
                "scope": "target",
                "status": "active",
                "verificationLevel": 0,
                "basisRefs": [],
            }
        ],
    }
    write_object(bundle / "evidence-ledger.json", evidence)

    decisions = {
        "schemaVersion": 1,
        "decisions": [
            {
                "id": f"DEC-track-{track}",
                "question": f"Is {track} a declared product track?",
                "choice": "Yes",
                "status": "accepted",
                "decidedBy": "self-test",
                "decidedAt": observed_at,
                "evidenceRefs": ["E-user-scope"],
            }
            for track in TRACKS
        ]
        + [
            {
                "id": "DEC-freeze",
                "question": "Is this fixture ready to freeze?",
                "choice": "Yes",
                "status": "accepted",
                "decidedBy": "self-test",
                "decidedAt": observed_at,
                "evidenceRefs": ["E-user-scope"],
            },
            {
                "id": "DEC-entitlement-free",
                "question": "Is the self-test capability free?",
                "choice": "Yes",
                "status": "accepted",
                "decidedBy": "self-test",
                "decidedAt": observed_at,
                "evidenceRefs": ["E-user-scope"],
            }
        ],
    }
    write_object(bundle / "decisions.json", decisions)

    requirements = {
        "schemaVersion": 1,
        "requirements": [
            {
                "id": "REQ-track-parity",
                "statement": "Both tracks expose every declared capability.",
                "scope": "target",
                "impact": "critical",
                "state": "confirmed",
                "owner": "learning",
                "evidenceRefs": ["E-user-scope"],
                "acceptance": ["governance validator passes"],
            }
        ],
    }
    write_object(bundle / "requirements.json", requirements)

    domains = {
        "schemaVersion": 1,
        "domains": [
            {
                "id": "learning",
                "owner": "product-architecture",
                "owns": ["learning-record"],
                "trackScope": TRACKS,
                "invariants": ["Track identity is never inferred from an empty value."],
                "commands": ["learning-create@v1"],
                "queries": ["evidence@v1"],
                "emits": ["learning-created@v1"],
                "consumes": [],
            }
        ],
    }
    write_object(bundle / "domains.json", domains)

    glossary = {
        "schemaVersion": 1,
        "terms": [
            {
                "id": "TERM-learning-record",
                "term": "learning record",
                "definition": "The canonical self-test learning resource.",
                "owner": "learning",
                "aliases": [],
            }
        ],
    }
    write_object(bundle / "glossary.json", glossary)

    contracts = []
    for contract_id, kind in (
        ("learning-create@v1", "command"),
        ("learning-created@v1", "event"),
        ("advisor-candidate@v1", "ai_candidate"),
    ):
        contracts.append(
            {
                "id": contract_id,
                "kind": kind,
                "owner": "learning",
                "consumers": ["home"],
                "schemaLocator": f"inline://contracts.json#/contracts/{contract_id}",
                "inputSchema": {
                    "type": "object",
                    "description": "Self-test input payload.",
                    "properties": {},
                    "required": [],
                    "additionalProperties": False,
                },
                "outputSchema": {
                    "type": "object",
                    "description": "Self-test output payload.",
                    "properties": {},
                    "required": [],
                    "additionalProperties": False,
                },
                "dataClasses": ["internal"],
                "observability": {
                    "correlation": "Require a correlation ID.",
                    "audit": "Record the contract ID and outcome.",
                    "metrics": "Count outcomes by contract and status.",
                },
                "authorization": "self-test actor",
                "idempotency": "idempotency key",
                "compatibility": "additive within v1",
                "errorSemantics": ["explicit_error"],
                "requirementRefs": ["REQ-track-parity"],
                "evidenceRefs": [],
                "status": "specified",
            }
        )
    command_contract = next(
        item for item in contracts if item["id"] == "learning-create@v1"
    )
    command_contract["transactionScopeResources"] = ["learning-record"]
    command_contract["transactionBoundary"] = (
        "One learning transaction CAS-updates the learning record and commits its outbox."
    )
    command_contract["transactionEmits"] = ["learning-created@v1"]
    for capability in policy["defaultTrackCapabilities"]:
        contracts.append(
            {
                "id": f"{capability}@v1",
                "kind": "query",
                "owner": "learning",
                "consumers": ["home"],
                "schemaLocator": f"inline://contracts.json#/contracts/{capability}@v1",
                "inputSchema": {
                    "type": "object",
                    "description": "Self-test capability query.",
                    "properties": {},
                    "required": [],
                    "additionalProperties": False,
                },
                "outputSchema": {
                    "type": "object",
                    "description": "Self-test capability response.",
                    "properties": {},
                    "required": [],
                    "additionalProperties": False,
                },
                "dataClasses": ["internal"],
                "observability": {
                    "correlation": "Require a correlation ID.",
                    "audit": "Record the contract ID and outcome.",
                    "metrics": "Count outcomes by contract and status.",
                },
                "authorization": "self-test actor",
                "idempotency": "read-only",
                "compatibility": "additive within v1",
                "errorSemantics": ["explicit_unavailable"],
                "requirementRefs": ["REQ-track-parity"],
                "evidenceRefs": [],
                "status": "specified",
            }
        )
    write_object(bundle / "contracts.json", {"schemaVersion": 1, "contracts": contracts})

    state_machines = {
        "schemaVersion": 1,
        "machines": [
            {
                "id": "SM-learning-record",
                "owner": "learning",
                "resource": "learning-record",
                "initialState": "new",
                "states": ["new", "active"],
                "terminalStates": ["active"],
                "transitions": [
                    {
                        "id": "activate",
                        "from": "new",
                        "to": "active",
                        "command": "learning-create@v1",
                        "guards": [],
                        "emits": ["learning-created@v1"],
                        "writesResources": ["learning-record"],
                        "idempotency": "same key returns the first result",
                        "concurrency": "serialize by learning record ID",
                    }
                ],
            }
        ],
    }
    write_object(bundle / "state-machines.json", state_machines)

    states = {
        name: {
            "behavior": f"Render explicit {name} truth.",
            "visibleTruth": f"Only the verified {name} state is visible.",
            "prohibitedClaims": ["Do not claim success without evidence."],
            "allowedActions": [],
            "freshness": "Bound to the fixture version.",
            "recovery": "Reload the verified fixture state.",
            "correlation": "Expose the fixture correlation ID for support and audit lookup.",
        }
        for name in policy["requiredPageStates"]
    }
    pages = {
        "schemaVersion": 1,
        "stateProfiles": [],
        "pages": [
            {
                "id": "home",
                "route": "pages/home/index",
                "purpose": "Show the verified learning state.",
                "trackScope": TRACKS,
                "dataOwners": ["learning"],
                "entryConditions": [],
                "actions": [],
                "exitRoutes": [],
                "permissions": [],
                "states": states,
            }
        ],
    }
    write_object(bundle / "page-matrix.json", pages)

    journeys = {
        "schemaVersion": 1,
        "actors": [
            {
                "id": "learner",
                "name": "Learner",
                "permissions": ["read own learning state"],
                "constraints": ["cannot read another learner"],
            }
        ],
        "journeys": [
            {
                "id": "J-learning",
                "actorRef": "learner",
                "outcome": "See an explicit dual-track state.",
                "requirementRefs": ["REQ-track-parity"],
                "steps": [
                    {
                        "id": "open-home",
                        "surfaceRef": "home",
                        "action": "Open home",
                        "expectedState": "Both tracks are explicit",
                        "failurePaths": ["explicit unavailable state"],
                    }
                ],
                "acceptanceCommands": ["self-test end-to-end verifier"],
            }
        ],
    }
    write_object(bundle / "journeys.json", journeys)

    coverage = {
        "schemaVersion": 1,
        "capabilities": [
            {
                "capability": capability,
                "tracks": {
                    track: {
                        "status": "specified",
                        "producer": "learning",
                        "consumers": ["home"],
                        "contract": f"{capability}@v1",
                        "evidenceRefs": ["E-user-scope"],
                        "decisionRef": f"DEC-track-{track}",
                    }
                    for track in TRACKS
                },
            }
            for capability in policy["defaultTrackCapabilities"]
        ],
    }
    write_object(bundle / "track-coverage.json", coverage)

    data_lifecycle = {
        "schemaVersion": 1,
        "resources": [
            {
                "id": "learning-record",
                "owner": "learning",
                "classification": "personal",
                "state": "confirmed",
                "purpose": "Run the governance self-test.",
                "source": "Self-test actor input.",
                "retention": "Delete at the end of the temporary test.",
                "deletion": "Temporary directory removal deletes the record.",
                "export": "JSON fixture export.",
                "encryption": "Temporary local filesystem only.",
                "residency": "Local temporary directory.",
                "evidenceRefs": ["E-user-scope"],
                "derivedFrom": [],
                "dataSubjects": ["self-test-actor"],
                "dataElements": ["fixture-record"],
                "processingPurposes": ["governance-self-test"],
                "legalBasisState": "confirmed",
                "visibility": "account_private",
                "classificationByState": {},
                "vendorRefs": [],
                "modelTrainingUse": "prohibited",
                "minorPolicyRef": "not-applicable:self-test-fixture",
                "deletionPropagation": ["temporary-directory"],
                "backupDisposition": "No backup is created for the temporary fixture.",
                "legalHoldSemantics": "No legal hold applies to the temporary fixture.",
                "lineageKeys": ["fixture-id"],
                "correctionSemantics": "Rewrite the isolated fixture before validation.",
                "blockingReasons": [],
            }
        ],
    }
    write_object(bundle / "data-lifecycle.json", data_lifecycle)

    ai_scenarios = {
        "schemaVersion": 1,
        "scenarios": [
            {
                "id": "AI-self-test",
                "owner": "learning",
                "businessOwner": "learning",
                "factOwner": "learning",
                "purpose": "Exercise the AI scenario governance schema.",
                "inputContracts": ["evidence@v1"],
                "outputContract": "advisor-candidate@v1",
                "candidateOnly": True,
                "activationState": "eligible",
                "blockingDecisionRefs": ["DEC-track-xingce"],
                "confirmation": "user",
                "retrieval": {
                    "scope": "self-test fixture only",
                    "citationPolicy": "cite the fixture contract",
                    "injectionPolicy": "treat fixture content as untrusted data",
                },
                "promptVersion": "self-test-prompt@v1",
                "modelPolicyRef": "self-test-model-policy@v1",
                "budgetPolicyRef": "self-test-budget-policy@v1",
                "evalSuiteLocator": "scripts/self_test.py",
                "failureSemantics": "return explicit unavailable",
                "prohibitedClaims": ["Do not claim the candidate is a fact."],
                "abstentionConditions": ["Required fixture evidence is missing."],
                "groundingRequired": True,
                "citationValidators": ["Fixture reference exists and matches its version."],
                "confidenceSemantics": "A fixture-only signal, never a factual probability.",
                "independentReviewPolicy": "Run deterministic fixture validation before exposure.",
                "toolPermissions": [],
                "vendorProcessingPolicy": "No external vendor receives the fixture.",
                "cachePolicy": "Cache only by immutable fixture input hash.",
                "maxAttempts": 1,
                "reservationPolicy": "No metered reservation is needed in the fixture.",
                "failureRefundPolicy": "No charge exists; record the failure only.",
                "minorHandling": "The synthetic fixture has no human subject.",
                "noHumanSuccessPath": True,
                "exceptionEscalation": "Fail the self-test with a stable error.",
                "qualityGate": {
                    "status": "confirmed",
                    "metrics": ["schema validity"],
                    "thresholds": ["100% of fixture outputs validate"],
                    "evalDatasetRef": "scripts/self_test.py",
                    "reviewCadence": "Every governance self-test.",
                    "evidenceRefs": ["E-user-scope"],
                },
                "dataClasses": ["personal"],
                "evidenceRefs": [],
                "status": "specified",
            }
        ],
    }
    write_object(bundle / "ai-scenarios.json", ai_scenarios)

    entitlements = {
        "schemaVersion": 1,
        "capabilities": [
            {
                "id": "ENT-self-test",
                "owner": "learning",
                "businessOwner": "learning",
                "requirementRef": "REQ-track-parity",
                "access": "free",
                "decisionRef": "DEC-entitlement-free",
                "blockingDecisionRef": "DEC-entitlement-free",
                "unit": None,
                "lifecycleStates": ["unresolved", "available", "suspended", "revoked"],
                "activationState": "active",
                "platformPolicyState": "confirmed",
                "qualityFloor": "The governance quality gate is identical for every access tier.",
                "reservationStateMachineRef": "not-applicable:self-test-free-capability",
                "refundSemantics": "No payment or refund exists in the self-test.",
                "contentLicenseDependency": "The synthetic fixture has no licensed content.",
                "measurementSource": "The deterministic self-test report.",
                "blockingReason": "No blocker remains in the accepted fixture decision.",
                "contractRefs": ["evidence@v1"],
            }
        ],
    }
    write_object(bundle / "entitlements.json", entitlements)

    nfr = {
        "schemaVersion": 1,
        "requirements": [
            {
                "id": f"NFR-{category}",
                "category": category,
                "statement": f"The {category} gate has an approved measurable target.",
                "scope": "target",
                "impact": "high",
                "state": "confirmed",
                "owner": "learning",
                "evidenceRefs": ["E-user-scope"],
                "acceptance": [f"verify {category}"],
                "targetDimensions": [f"observable {category} behavior"],
                "blockingDecisionRef": "DEC-track-xingce",
                "proposedVerification": {
                    "condition": f"The confirmed {category} target is exercised.",
                    "method": f"Run the deterministic {category} fixture check.",
                    "environment": "Temporary isolated Git repository.",
                    "dataVolume": "One deterministic fixture.",
                    "evidenceArtifact": f"evidence/{category}.json",
                    "minimumEvidenceLevel": policy["nfrMinimumEvidenceLevels"][category],
                    "reviewCadence": "Every governance freeze.",
                },
                "verification": {
                    "status": "planned",
                    "condition": f"The self-test {category} condition is observable.",
                    "method": f"Run the self-test {category} check.",
                    "environment": "Temporary isolated Git repository.",
                    "dataVolume": "One deterministic fixture.",
                    "evidenceArtifact": f"evidence/{category}.json",
                    "minimumEvidenceLevel": policy["nfrMinimumEvidenceLevels"][category],
                    "reviewCadence": "Every governance freeze.",
                },
            }
            for category in policy["requiredNfrCategories"]
        ],
    }
    write_object(bundle / "nfr.json", nfr)

    scenario_id = "WP-self-AS-01"
    scenario_selector = "tests/work_packages/wp-self/test_acceptance.py::test_wp_self_as_01"
    scenario_command_id = "wp-self-as-01-execute"
    scenario_assertion = "duplicate delivery, a stale revision and recovery converge without duplicate canonical facts"
    test_source = (
        "import os\n"
        "\n"
        "def test_wp_self_as_01():\n"
        f"    assert os.environ['GOVERNANCE_SCENARIO_ID'] == {scenario_id!r}\n"
        f"    assert os.environ['GOVERNANCE_ACCEPTANCE_COMMAND_ID'] == {scenario_command_id!r}\n"
        "    assert os.environ['GOVERNANCE_RUN_ID']\n"
        "    authority_hash = os.environ.get('GOVERNANCE_RELEASE_UNIT_HASH') or os.environ.get('GOVERNANCE_MANIFEST_HASH')\n"
        "    assert authority_hash.startswith('sha256:')\n"
        "    if os.environ.get('GOVERNANCE_RELEASE_UNIT_HASH'):\n"
        "        assert os.environ['GOVERNANCE_RELEASE_UNIT_ID']\n"
    )
    test_path = root / "tests" / "work_packages" / "wp-self" / "test_acceptance.py"
    test_path.parent.mkdir(parents=True, exist_ok=True)
    test_path.write_text(test_source, encoding="utf-8")
    packages = {
        "schemaVersion": 1,
        "packages": [
            {
                "id": "WP-contract",
                "wave": "contract",
                "owner": "learning",
                "riskClass": "low",
                "objective": "Create the frozen test contract.",
                "deliverables": ["A deterministic self-test contract fixture."],
                "requirementRefs": ["REQ-track-parity"],
                "decisionRefs": ["DEC-track-xingce"],
                "blockingDecisionRefs": [],
                "nonGoals": ["Do not modify product behavior outside the fixture."],
                "dependsOn": [],
                "allowedPaths": ["contracts/**", "tests/work_packages/wp-self/**"],
                "forbiddenPaths": ["migrations/**", ".product-governance/**"],
                "migrationPolicy": {
                    "mode": "none",
                    "ownedPaths": [],
                    "dataBackfill": "No data migration is required by the fixture.",
                    "rollback": "Remove only the generated fixture contract.",
                },
                "compatibilityPolicy": {
                    "strategy": "Additive changes only within v1.",
                    "supportedVersions": "v1",
                    "removalGate": "A superseding fixture decision and a new freeze are required.",
                },
                "inputContracts": ["advisor-candidate@v1"],
                "outputContracts": ["learning-create@v1"],
                "outputArtifacts": [
                    {
                        "id": "learning-contract-codegen@v1",
                        "kind": "generated_source_bundle",
                        "generatorPath": "contracts/codegen.py",
                        "manifestPath": "contracts/generated/manifest.json",
                        "outputPaths": ["contracts/generated/**"],
                        "sourceContractRefs": ["advisor-candidate@v1"],
                        "generatorVersion": "v1",
                        "deterministic": True,
                        "verificationScenarioId": scenario_id,
                    }
                ],
                "acceptanceCommands": [
                    {
                        "id": scenario_command_id,
                        "kind": "scenario_test",
                        "adapter": "pytest-json@1",
                        "selector": scenario_selector,
                        "cwd": ".",
                        "timeoutSeconds": 30,
                    }
                ],
                "requiredAcceptanceScenarios": [
                    {
                        "id": scenario_id,
                        "assertion": scenario_assertion,
                        "testSelector": scenario_selector,
                        "acceptanceCommandId": scenario_command_id,
                    }
                ],
                "verificationOnly": False,
                "stopConditions": ["Contract evidence is contradictory."],
                "status": "planned",
            }
        ],
    }
    verification_selector = (
        "tests/work_packages/wp-verify-only/test_acceptance.py::"
        "test_wp_verify_only_as_01"
    )
    verification_test_path = (
        root
        / "tests"
        / "work_packages"
        / "wp-verify-only"
        / "test_acceptance.py"
    )
    verification_test_path.parent.mkdir(parents=True, exist_ok=True)
    verification_test_path.write_text(
        "import os\n\n"
        "def test_wp_verify_only_as_01():\n"
        "    assert os.environ['GOVERNANCE_SCENARIO_ID'] == 'WP-verify-only-AS-01'\n"
        "    assert os.environ['GOVERNANCE_RUN_ID']\n",
        encoding="utf-8",
    )
    verification_package = json.loads(json.dumps(packages["packages"][0]))
    verification_package.update(
        {
            "id": "WP-verify-only",
            "wave": "release",
            "objective": "Verify the frozen source tree without modifying project source.",
            "deliverables": ["A strictly read-only verification report."],
            "dependsOn": [],
            "allowedPaths": ["tests/work_packages/wp-verify-only/**"],
            "inputContracts": ["learning-create@v1"],
            "outputContracts": [],
            "acceptanceCommands": [
                {
                    "id": "wp-verify-only-as-01-execute",
                    "kind": "scenario_test",
                    "adapter": "pytest-json@1",
                    "selector": verification_selector,
                    "cwd": ".",
                    "timeoutSeconds": 30,
                }
            ],
            "requiredAcceptanceScenarios": [
                {
                    "id": "WP-verify-only-AS-01",
                    "assertion": "the verification package reads the frozen source tree without changing any project source file",
                    "testSelector": verification_selector,
                    "acceptanceCommandId": "wp-verify-only-as-01-execute",
                }
            ],
            "verificationOnly": True,
        }
    )
    verification_package.pop("outputArtifacts", None)
    packages["packages"].append(verification_package)
    write_object(bundle / "work-packages.json", packages)

    manifest = read_object(bundle / "manifest.json")
    manifest["status"] = "review"
    manifest["trackDecisionRefs"] = {
        track: f"DEC-track-{track}" for track in TRACKS
    }
    write_object(bundle / "manifest.json", manifest)


def exercise_negative_gates(root: Path) -> None:
    bundle = root / BUNDLE_DIR

    evidence_path = bundle / "evidence-ledger.json"
    evidence = read_object(evidence_path)
    evidence["entries"].append(
        {
            "id": "E-unsupported-inference",
            "claim": "An unsupported inference must fail.",
            "class": "inferred",
            "locator": "analysis:self-test",
            "observedAt": "2026-01-01T00:00:00+00:00",
            "scope": "proposal",
            "status": "active",
            "verificationLevel": 0,
            "basisRefs": [],
        }
    )
    write_object(evidence_path, evidence)
    assert_error(root, "review", "requires basisRefs")
    evidence["entries"].pop()
    write_object(evidence_path, evidence)

    evidence["entries"].append(
        {
            "id": "E-inferred-capacity",
            "claim": "The product supports an invented concurrency threshold.",
            "class": "inferred",
            "locator": "analysis:self-test:invented-threshold",
            "observedAt": "2026-01-01T00:00:00+00:00",
            "scope": "proposal",
            "status": "active",
            "verificationLevel": 0,
            "basisRefs": ["E-user-scope"],
        }
    )
    write_object(evidence_path, evidence)
    nfr_path = bundle / "nfr.json"
    nfr = read_object(nfr_path)
    original_nfr_refs = list(nfr["requirements"][0]["evidenceRefs"])
    nfr["requirements"][0]["evidenceRefs"] = ["E-inferred-capacity"]
    write_object(nfr_path, nfr)
    assert_error(root, "review", "requires user-confirmed or external evidence")
    nfr["requirements"][0]["evidenceRefs"] = original_nfr_refs
    write_object(nfr_path, nfr)
    evidence["entries"].pop()
    write_object(evidence_path, evidence)

    nfr["requirements"][0]["verification"]["status"] = "verified"
    write_object(nfr_path, nfr)
    assert_error(root, "review", "verified lacks evidence at level")
    nfr["requirements"][0]["verification"]["status"] = "planned"
    write_object(nfr_path, nfr)

    evidence["entries"].append(
        {
            "id": "E-fake-test-promotion",
            "claim": "A claimed test result without an artifact must fail.",
            "class": "code_observed",
            "locator": "command:self-test:false-claim",
            "observedAt": "2026-01-01T00:00:00+00:00",
            "scope": "current",
            "status": "active",
            "verificationLevel": 2,
            "basisRefs": [],
        }
    )
    write_object(evidence_path, evidence)
    assert_error(root, "review", "requires a hashed artifact")
    evidence["entries"].pop()
    write_object(evidence_path, evidence)

    package_path = bundle / "work-packages.json"
    packages = read_object(package_path)
    output_artifact = packages["packages"][0]["outputArtifacts"][0]
    original_source_contract_refs = list(output_artifact["sourceContractRefs"])
    output_artifact["sourceContractRefs"] = ["learning-create@v1"]
    write_object(package_path, packages)
    assert_error(root, "review", "references contract outside inputContracts")
    output_artifact["sourceContractRefs"] = original_source_contract_refs
    original_output_paths = list(output_artifact["outputPaths"])
    output_artifact["outputPaths"] = ["outside-owned-path/**"]
    write_object(package_path, packages)
    assert_error(root, "review", "outputPaths is outside allowedPaths")
    output_artifact["outputPaths"] = original_output_paths
    write_object(package_path, packages)
    scenarios = packages["packages"][0].pop("requiredAcceptanceScenarios")
    write_object(package_path, packages)
    assert_error(root, "review", "requiredAcceptanceScenarios must be non-empty")
    packages["packages"][0]["requiredAcceptanceScenarios"] = scenarios
    write_object(package_path, packages)
    packages["packages"][0]["requiredAcceptanceScenarios"] = ["prose alone must not pass"]
    write_object(package_path, packages)
    assert_error(root, "review", "requiredAcceptanceScenarios[0] must be an object")
    packages["packages"][0]["requiredAcceptanceScenarios"] = scenarios
    write_object(package_path, packages)
    missing_selector = packages["packages"][0]["requiredAcceptanceScenarios"][0].pop("testSelector")
    write_object(package_path, packages)
    assert_error(root, "review", "requiredAcceptanceScenarios[0] is missing fields")
    packages["packages"][0]["requiredAcceptanceScenarios"][0]["testSelector"] = missing_selector
    write_object(package_path, packages)
    scenario = packages["packages"][0]["requiredAcceptanceScenarios"][0]
    scenario_command = next(
        item
        for item in packages["packages"][0]["acceptanceCommands"]
        if item["id"] == scenario["acceptanceCommandId"]
    )
    original_scenario_command = json.loads(json.dumps(scenario_command))
    scenario_command["adapter"] = "shell-output@1"
    write_object(package_path, packages)
    assert_error(root, "review", "adapter must be pytest-json@1")
    scenario_command.clear()
    scenario_command.update(json.loads(json.dumps(original_scenario_command)))
    scenario_command["argv"] = ["/usr/bin/true", scenario["testSelector"]]
    write_object(package_path, packages)
    assert_error(root, "review", "scenario command has unsupported fields")
    scenario_command.clear()
    scenario_command.update(json.loads(json.dumps(original_scenario_command)))
    scenario_command["selector"] = (
        f"unrelated.py --ignore {scenario['testSelector']}"
    )
    write_object(package_path, packages)
    assert_error(root, "review", "does not execute testSelector")
    scenario_command.clear()
    scenario_command.update(json.loads(json.dumps(original_scenario_command)))
    scenario["evidenceArtifact"] = "outside-package.txt"
    packages["packages"][0]["evidenceArtifacts"] = ["outside-package.txt"]
    write_object(package_path, packages)
    assert_error(root, "review", "evidenceArtifacts is forbidden")
    scenario.pop("evidenceArtifact")
    packages["packages"][0].pop("evidenceArtifacts")
    write_object(package_path, packages)
    forged_runner_result = {
        "passed": True,
        "actualExitCode": 0,
        "timedOut": False,
        "stdout": "Tests 1 passed",
        "stderr": "",
        "runnerProof": {
            "adapter": "pytest-json@1",
            "selector": scenario_command["selector"],
        },
    }
    if verify_scenario_result(scenario_command, forged_runner_result) is not None:
        raise AssertionError("Forged stdout was accepted without a machine runner report")

    fake_run_id = "WP-contract@forged"
    fake_manifest_hash = "sha256:" + ("1" * 64)
    fake_report_relative = report_relative_path(fake_run_id, "WP-contract")
    fake_report_path = bundle_execution_path(root, fake_report_relative)
    write_object(
        fake_report_path,
        {
            "schemaVersion": 2,
            "runId": fake_run_id,
            "packageId": "WP-contract",
            "manifestHash": fake_manifest_hash,
            "passed": True,
            "testedTreeHash": "sha256:" + ("2" * 64),
            "ownedResultHashes": {},
            "commands": [],
            "scenarioEvidence": [],
        },
    )
    fake_run = {
        "id": fake_run_id,
        "packageId": "WP-contract",
        "manifestHash": fake_manifest_hash,
        "status": "completed",
        "reportPath": fake_report_relative,
        "reportSha256": sha256(fake_report_path),
    }
    if completed_run_is_verified(
        root, fake_run, packages["packages"][0], fake_manifest_hash
    ):
        raise AssertionError("A shallow self-declared dependency report was accepted")
    duplicate: dict[str, Any] = json.loads(json.dumps(packages["packages"][0]))
    duplicate["id"] = "WP-overlap"
    packages["packages"].append(duplicate)
    write_object(package_path, packages)
    assert_error(root, "review", "Concurrently runnable work-package paths overlap")
    packages["packages"].pop()
    write_object(package_path, packages)

    manifest_path = bundle / "manifest.json"
    manifest = read_object(manifest_path)
    original_states = list(manifest["requiredPageStates"])
    manifest["requiredPageStates"] = original_states[:-1]
    write_object(manifest_path, manifest)
    assert_error(root, "review", "cannot omit policy requirements")
    manifest["requiredPageStates"] = original_states
    write_object(manifest_path, manifest)

    contracts_path = bundle / "contracts.json"
    contracts = read_object(contracts_path)
    command_contract = next(
        item for item in contracts["contracts"] if item["id"] == "learning-create@v1"
    )
    original_description = command_contract["inputSchema"]["description"]
    command_contract["inputSchema"]["description"] = "Draft command envelope placeholder."
    write_object(contracts_path, contracts)
    assert_error(root, "review", "schema contains draft placeholder language")
    command_contract["inputSchema"]["description"] = original_description
    write_object(contracts_path, contracts)

    original_input_schema = json.loads(json.dumps(command_contract["inputSchema"]))
    ambiguous_branch = json.loads(json.dumps(original_input_schema))
    command_contract["inputSchema"]["oneOf"] = [
        ambiguous_branch,
        json.loads(json.dumps(ambiguous_branch)),
    ]
    write_object(contracts_path, contracts)
    assert_error(root, "review", "oneOf branches 0 and 1 are not provably disjoint")
    command_contract["inputSchema"] = original_input_schema
    write_object(contracts_path, contracts)

    original_conditional_schema = json.loads(json.dumps(command_contract["inputSchema"]))
    first_branch = {
        "type": "object",
        "description": "First satisfiable conditional branch.",
        "properties": {
            "kind": {
                "type": "string",
                "description": "First branch discriminator.",
                "enum": ["first"],
            }
        },
        "required": ["kind"],
        "additionalProperties": False,
    }
    second_branch = json.loads(json.dumps(first_branch))
    second_branch["description"] = "Second satisfiable conditional branch."
    second_branch["properties"]["kind"]["description"] = "Second branch discriminator."
    second_branch["properties"]["kind"]["enum"] = ["second"]
    command_contract["inputSchema"] = {
        "type": "object",
        "description": "An invalid parent that silently rules out the first branch.",
        "properties": {
            "kind": {
                "type": "string",
                "description": "Incorrectly copied last-branch discriminator.",
                "enum": ["second"],
            }
        },
        "required": [],
        "additionalProperties": False,
        "oneOf": [first_branch, second_branch],
    }
    write_object(contracts_path, contracts)
    assert_error(root, "review", "narrows or contradicts oneOf[0]")
    command_contract["inputSchema"] = original_conditional_schema
    write_object(contracts_path, contracts)

    mismatched_type_schema = json.loads(json.dumps(original_conditional_schema))
    mismatched_type_schema["oneOf"] = [
        {
            "type": "string",
            "description": "Incorrect scalar branch under an object parent.",
            "const": "first",
        },
        {
            "type": "string",
            "description": "Second incorrect scalar branch under an object parent.",
            "const": "second",
        },
    ]
    command_contract["inputSchema"] = mismatched_type_schema
    write_object(contracts_path, contracts)
    assert_error(root, "review", "type must equal parent type object")
    command_contract["inputSchema"] = original_conditional_schema
    write_object(contracts_path, contracts)

    unsupported_keyword_schema = json.loads(json.dumps(original_conditional_schema))
    unsupported_keyword_schema["not"] = {
        "type": "object",
        "description": "Unsupported hidden constraint.",
        "properties": {},
        "required": [],
        "additionalProperties": False,
    }
    command_contract["inputSchema"] = unsupported_keyword_schema
    write_object(contracts_path, contracts)
    assert_error(root, "review", "uses unsupported JSON Schema keywords: not")
    command_contract["inputSchema"] = original_conditional_schema
    write_object(contracts_path, contracts)

    numeric_parent = {
        "type": "object",
        "description": "Numeric JSON equality must not prove branches disjoint.",
        "properties": {
            "value": {
                "type": "number",
                "description": "Numeric discriminator under test.",
            }
        },
        "required": [],
        "additionalProperties": False,
    }
    numeric_first = {
        "type": "object",
        "description": "Integer spelling of one.",
        "properties": {
            "value": {
                "type": "number",
                "description": "Numeric const one.",
                "const": 1,
            }
        },
        "required": ["value"],
        "additionalProperties": False,
    }
    numeric_second = json.loads(json.dumps(numeric_first))
    numeric_second["description"] = "Decimal spelling of one."
    numeric_second["properties"]["value"]["const"] = 1.0
    numeric_parent["oneOf"] = [numeric_first, numeric_second]
    command_contract["inputSchema"] = numeric_parent
    write_object(contracts_path, contracts)
    assert_error(root, "review", "oneOf branches 0 and 1 are not provably disjoint")
    command_contract["inputSchema"] = original_conditional_schema
    write_object(contracts_path, contracts)

    parent_required_schema = json.loads(json.dumps(original_conditional_schema))
    parent_required_schema["properties"] = {
        "kind": {
            "type": "string",
            "description": "Conditional discriminator.",
            "enum": ["first", "second"],
        },
        "parentReceipt": {
            "type": "string",
            "description": "A parent-required field omitted and forbidden by every branch.",
        },
    }
    parent_required_schema["required"] = ["parentReceipt"]
    parent_required_schema["oneOf"] = [first_branch, second_branch]
    command_contract["inputSchema"] = parent_required_schema
    write_object(contracts_path, contracts)
    assert_error(root, "review", "forbids parent-required properties: parentReceipt")
    command_contract["inputSchema"] = original_conditional_schema
    write_object(contracts_path, contracts)

    original_scope = command_contract.pop("transactionScopeResources")
    original_boundary = command_contract.pop("transactionBoundary")
    write_object(contracts_path, contracts)
    assert_error(root, "review", "must declare transactionScopeResources exactly matching")
    command_contract["transactionScopeResources"] = original_scope
    command_contract["transactionBoundary"] = original_boundary
    write_object(contracts_path, contracts)

    original_emits = command_contract.pop("transactionEmits")
    write_object(contracts_path, contracts)
    assert_error(root, "review", "must declare transactionEmits exactly matching")
    command_contract["transactionEmits"] = []
    write_object(contracts_path, contracts)
    assert_error(root, "review", "must exactly match the state-machine emits union")
    command_contract["transactionEmits"] = original_emits
    write_object(contracts_path, contracts)

    machines_path = bundle / "state-machines.json"
    machines = read_object(machines_path)
    machines["machines"][0]["transitions"].append(
        {
            "id": "illegal-terminal-reopen",
            "from": "active",
            "to": "active",
            "command": "learning-create@v1",
            "guards": [],
            "emits": [],
            "writesResources": ["learning-record"],
            "idempotency": "same key returns the first result",
            "concurrency": "serialize by learning record ID",
        }
    )
    write_object(machines_path, machines)
    assert_error(root, "review", "terminal state active has an outbound transition")
    machines["machines"][0]["transitions"].pop()
    write_object(machines_path, machines)

    machines["machines"][0]["states"].append("orphan_terminal")
    machines["machines"][0]["terminalStates"].append("orphan_terminal")
    write_object(machines_path, machines)
    assert_error(root, "review", "has unreachable states")
    machines["machines"][0]["states"].pop()
    machines["machines"][0]["terminalStates"].pop()
    write_object(machines_path, machines)

    domains_path = bundle / "domains.json"
    domains = read_object(domains_path)
    domains["domains"][0]["commands"].remove("learning-create@v1")
    write_object(domains_path, domains)
    assert_error(root, "review", "is not owned by domain learning")
    domains["domains"][0]["commands"].append("learning-create@v1")
    write_object(domains_path, domains)

    domains["domains"][0]["emits"].remove("learning-created@v1")
    write_object(domains_path, domains)
    assert_error(root, "review", "not declared by owner domain learning")
    domains["domains"][0]["emits"].append("learning-created@v1")
    write_object(domains_path, domains)

    lifecycle_path = bundle / "data-lifecycle.json"
    lifecycle = read_object(lifecycle_path)
    domains["domains"][0]["owns"].append("learning-summary")
    second_lifecycle = json.loads(json.dumps(lifecycle["resources"][0]))
    second_lifecycle["id"] = "learning-summary"
    lifecycle["resources"].append(second_lifecycle)
    machines["machines"][0]["transitions"][0]["writesResources"] = [
        "learning-record",
        "learning-summary",
    ]
    write_object(domains_path, domains)
    write_object(machines_path, machines)
    write_object(lifecycle_path, lifecycle)
    assert_error(root, "review", "must exactly match the state-machine writesResources union")
    command_contract["transactionScopeResources"] = [
        "learning-record",
        "learning-summary",
    ]
    command_contract["transactionBoundary"] = (
        "One self-test transaction writes both canonical resources and its outbox."
    )
    write_object(contracts_path, contracts)
    assert_valid(root, "review")
    machines["machines"][0]["transitions"][0]["writesResources"] = [
        "learning-record"
    ]
    write_object(machines_path, machines)
    assert_error(root, "review", "must exactly match the state-machine writesResources union")
    machines["machines"][0]["transitions"][0]["writesResources"] = [
        "learning-record",
        "learning-summary",
    ]
    command_contract["transactionScopeResources"] = ["learning-record"]
    command_contract["transactionBoundary"] = (
        "One learning transaction CAS-updates the learning record and commits its outbox."
    )
    write_object(contracts_path, contracts)
    domains["domains"][0]["owns"].pop()
    lifecycle["resources"].pop()
    machines["machines"][0]["transitions"][0]["writesResources"] = [
        "learning-record"
    ]
    write_object(domains_path, domains)
    write_object(machines_path, machines)
    write_object(lifecycle_path, lifecycle)

    domains["domains"][0]["consumes"].append("learning-created@v1")
    write_object(domains_path, domains)
    assert_error(root, "review", "without an explicit selfConsumptionRationale")
    domains["domains"][0]["consumes"].pop()
    write_object(domains_path, domains)


def seal_and_detect_tampering(root: Path) -> None:
    script = Path(__file__).resolve().parent / "seal_governance.py"
    result = subprocess.run(
        [
            sys.executable,
            str(script),
            "--project-root",
            str(root),
            "--decision-id",
            "DEC-freeze",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise AssertionError(f"Sealing failed: {result.stdout}\n{result.stderr}")
    assert_valid(root, "freeze")

    scripts = Path(__file__).resolve().parent
    test_path = root / "tests" / "work_packages" / "wp-self" / "test_acceptance.py"
    original_test_source = test_path.read_text(encoding="utf-8")
    implementation_path = root / "contracts" / "learning-implementation.txt"

    readonly_dispatch = subprocess.run(
        [
            sys.executable,
            str(scripts / "dispatch_work_package.py"),
            "--project-root",
            str(root),
            "--package-id",
            "WP-contract",
            "--assigned-to",
            "readonly-negative-agent",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if readonly_dispatch.returncode != 0:
        raise AssertionError(
            f"Read-only negative dispatch failed: {readonly_dispatch.stdout}\n{readonly_dispatch.stderr}"
        )
    implementation_path.parent.mkdir(parents=True, exist_ok=True)
    implementation_path.write_text(
        "failed-run source left for the next governed baseline\n", encoding="utf-8"
    )
    protected_content = implementation_path.read_text(encoding="utf-8")
    test_path.write_text(
        "import os\n"
        "from pathlib import Path\n\n"
        "def test_wp_self_as_01():\n"
        "    target = Path(os.environ['GOVERNANCE_PROJECT_ROOT']) / 'contracts' / 'learning-implementation.txt'\n"
        "    original = target.read_text(encoding='utf-8')\n"
        "    try:\n"
        "        target.write_text('temporary forged implementation\\n', encoding='utf-8')\n"
        "    finally:\n"
        "        target.write_text(original, encoding='utf-8')\n",
        encoding="utf-8",
    )
    readonly_verify = subprocess.run(
        [
            sys.executable,
            str(scripts / "verify_work_package.py"),
            "--project-root",
            str(root),
            "--run-id",
            readonly_dispatch.stdout.strip(),
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if readonly_verify.returncode == 0:
        raise AssertionError("A test that temporarily rewrote source bypassed read-only isolation")
    if implementation_path.read_text(encoding="utf-8") != protected_content:
        raise AssertionError("Read-only acceptance isolation allowed source bytes to change")
    test_path.write_text(original_test_source, encoding="utf-8")
    assert_valid(root, "freeze")

    outside_owned_state = root / "outside-owned-state.txt"
    outside_owned_state.write_text(
        "present during acceptance but outside package ownership\n", encoding="utf-8"
    )
    dispatch = subprocess.run(
        [
            sys.executable,
            str(scripts / "dispatch_work_package.py"),
            "--project-root",
            str(root),
            "--package-id",
            "WP-contract",
            "--assigned-to",
            "self-test-agent",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if dispatch.returncode != 0:
        raise AssertionError(f"Dispatch failed: {dispatch.stdout}\n{dispatch.stderr}")
    run_id = dispatch.stdout.strip()
    implementation_path.write_text(
        "implemented after the governed baseline\n", encoding="utf-8"
    )
    verify = subprocess.run(
        [
            sys.executable,
            str(scripts / "verify_work_package.py"),
            "--project-root",
            str(root),
            "--run-id",
            run_id,
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if verify.returncode != 0:
        raise AssertionError(f"Verification failed: {verify.stdout}\n{verify.stderr}")
    registry = read_object(
        root / BUNDLE_DIR / "execution" / "work-package-runs.json"
    )
    completed_run = next(item for item in registry["runs"] if item["id"] == run_id)
    if completed_run["status"] != "completed":
        raise AssertionError("Acceptance execution did not complete the work-package run")
    scenario_relative = scenario_evidence_relative_path(
        run_id, "WP-contract", "WP-self-AS-01"
    )
    scenario_payload = read_object(bundle_execution_path(root, scenario_relative))
    if scenario_payload.get("evidenceOwner") != "govern-product-build/verifier":
        raise AssertionError("Scenario evidence was not verifier-issued")
    completed_report = read_object(root / BUNDLE_DIR / completed_run["reportPath"])
    if not completed_report.get("testedTreeHash") or completed_report.get("sourceScopeFailures"):
        raise AssertionError("Completed report is not bound to a clean tested source tree")
    tested_owned_hashes = completed_report.get("testedOwnedState", {}).get("pathHashes", {})
    if not {
        "contracts/learning-implementation.txt",
        "tests/work_packages/wp-self/test_acceptance.py",
    }.issubset(tested_owned_hashes):
        raise AssertionError(
            "Completed report omitted a changed output or baseline-existing owned file"
        )
    if "outside-owned-state.txt" in tested_owned_hashes:
        raise AssertionError("Completed report captured an out-of-scope file")
    assert_valid(root, "freeze")

    completed_report["schemaVersion"] = 2
    completed_report.pop("testedOwnedState")
    completed_report.pop("testedOwnedStateSha256")
    completed_report_path = root / BUNDLE_DIR / completed_run["reportPath"]
    write_object(completed_report_path, completed_report)
    completed_run["reportSha256"] = sha256(completed_report_path)
    write_object(
        root / BUNDLE_DIR / "execution" / "work-package-runs.json", registry
    )
    assert_valid(root, "freeze")

    test_path.write_text(original_test_source + "\n# drift after successful verification\n", encoding="utf-8")
    assert_error(
        root,
        "freeze",
        "completed tested owned state changed after verification: tests/work_packages/wp-self/test_acceptance.py",
    )
    packages = read_object(root / BUNDLE_DIR / "work-packages.json")
    package = next(item for item in packages["packages"] if item["id"] == "WP-contract")
    if completed_run_is_verified(
        root,
        completed_run,
        package,
        completed_run["manifestHash"],
    ):
        raise AssertionError("Dependency verification accepted baseline-owned source drift")
    test_path.write_text(original_test_source, encoding="utf-8")
    assert_valid(root, "freeze")

    verified_output = implementation_path.read_text(encoding="utf-8")
    implementation_path.write_text(
        "changed after successful verification\n", encoding="utf-8"
    )
    assert_error(
        root,
        "freeze",
        "completed tested owned state changed after verification: contracts/learning-implementation.txt",
    )
    implementation_path.write_text(verified_output, encoding="utf-8")
    assert_valid(root, "freeze")

    outside_owned_state.write_text(
        "changed after verification but still outside package ownership\n",
        encoding="utf-8",
    )
    assert_valid(root, "freeze")

    verification_test_path = (
        root
        / "tests"
        / "work_packages"
        / "wp-verify-only"
        / "test_acceptance.py"
    )
    verification_test_source = verification_test_path.read_text(encoding="utf-8")
    verification_dispatch = subprocess.run(
        [
            sys.executable,
            str(scripts / "dispatch_work_package.py"),
            "--project-root",
            str(root),
            "--package-id",
            "WP-verify-only",
            "--assigned-to",
            "verification-mutation-negative-agent",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if verification_dispatch.returncode != 0:
        raise AssertionError(
            f"Verification-only negative dispatch failed: {verification_dispatch.stdout}\n{verification_dispatch.stderr}"
        )
    verification_test_path.write_text(
        verification_test_source + "\n# mutation before self-verification\n",
        encoding="utf-8",
    )
    verification_mutation = subprocess.run(
        [
            sys.executable,
            str(scripts / "verify_work_package.py"),
            "--project-root",
            str(root),
            "--run-id",
            verification_dispatch.stdout.strip(),
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if verification_mutation.returncode == 0:
        raise AssertionError(
            "A verification-only package changed its test source and self-verified"
        )
    verification_test_path.write_text(verification_test_source, encoding="utf-8")
    registry = read_object(
        root / BUNDLE_DIR / "execution" / "work-package-runs.json"
    )
    mutation_run = next(
        item
        for item in registry["runs"]
        if item["id"] == verification_dispatch.stdout.strip()
    )
    mutation_report = read_object(root / BUNDLE_DIR / mutation_run["reportPath"])
    if "verification-only work package changed project source" not in mutation_report.get(
        "integrityFailures", []
    ):
        raise AssertionError("Verification-only source mutation was not recorded")
    assert_valid(root, "freeze")

    readonly_verification_dispatch = subprocess.run(
        [
            sys.executable,
            str(scripts / "dispatch_work_package.py"),
            "--project-root",
            str(root),
            "--package-id",
            "WP-verify-only",
            "--assigned-to",
            "verification-readonly-agent",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if readonly_verification_dispatch.returncode != 0:
        raise AssertionError(
            f"Read-only verification dispatch failed: {readonly_verification_dispatch.stdout}\n{readonly_verification_dispatch.stderr}"
        )
    readonly_verification = subprocess.run(
        [
            sys.executable,
            str(scripts / "verify_work_package.py"),
            "--project-root",
            str(root),
            "--run-id",
            readonly_verification_dispatch.stdout.strip(),
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if readonly_verification.returncode != 0:
        raise AssertionError(
            f"Pure read-only verification failed: {readonly_verification.stdout}\n{readonly_verification.stderr}"
        )
    registry = read_object(
        root / BUNDLE_DIR / "execution" / "work-package-runs.json"
    )
    readonly_run = next(
        item
        for item in registry["runs"]
        if item["id"] == readonly_verification_dispatch.stdout.strip()
    )
    readonly_report = read_object(root / BUNDLE_DIR / readonly_run["reportPath"])
    if readonly_report.get("sourceDiff") != {}:
        raise AssertionError("Read-only verification reported a project source diff")
    assert_valid(root, "freeze")

    second_dispatch = subprocess.run(
        [
            sys.executable,
            str(scripts / "dispatch_work_package.py"),
            "--project-root",
            str(root),
            "--package-id",
            "WP-contract",
            "--assigned-to",
            "scope-negative-agent",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if second_dispatch.returncode != 0:
        raise AssertionError(
            f"Second dispatch failed: {second_dispatch.stdout}\n{second_dispatch.stderr}"
        )
    outside_path = root / "outside-package.txt"
    outside_path.write_text("not owned by WP-contract\n", encoding="utf-8")
    scope_verify = subprocess.run(
        [
            sys.executable,
            str(scripts / "verify_work_package.py"),
            "--project-root",
            str(root),
            "--run-id",
            second_dispatch.stdout.strip(),
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if scope_verify.returncode == 0:
        raise AssertionError("Out-of-scope source mutation incorrectly passed verification")
    outside_path.unlink()
    registry = read_object(
        root / BUNDLE_DIR / "execution" / "work-package-runs.json"
    )
    failed_run = next(
        item for item in registry["runs"] if item["id"] == second_dispatch.stdout.strip()
    )
    failed_report = read_object(root / BUNDLE_DIR / failed_run["reportPath"])
    if not any("outside package ownership" in item for item in failed_report["sourceScopeFailures"]):
        raise AssertionError("Out-of-scope mutation was not recorded in the failed report")
    assert_valid(root, "freeze")

    requirements_path = root / BUNDLE_DIR / "requirements.json"
    requirements = read_object(requirements_path)
    requirements["requirements"][0]["statement"] = "Tampered after freeze."
    write_object(requirements_path, requirements)
    assert_error(root, "freeze", "Governance hash mismatch")


def exercise_release_unit_flow(root: Path) -> None:
    bundle = root / BUNDLE_DIR
    manifest_path = bundle / "manifest.json"
    manifest = read_object(manifest_path)
    manifest["status"] = "draft"
    manifest["freeze"] = {
        "decisionId": None,
        "commit": None,
        "frozenAt": None,
        "manifestHash": None,
    }
    write_object(manifest_path, manifest)

    decisions_path = bundle / "decisions.json"
    decisions = read_object(decisions_path)
    decisions["decisions"].append(
        {
            "id": "DEC-release-unit",
            "question": "May the contract implementation release unit be sealed?",
            "choice": "Yes, without claiming full-product readiness.",
            "status": "accepted",
            "decidedBy": "self-test",
            "decidedAt": "2026-01-01T00:00:00+00:00",
            "evidenceRefs": ["E-user-scope"],
        }
    )
    write_object(decisions_path, decisions)

    requirements_path = bundle / "requirements.json"
    requirements = read_object(requirements_path)
    requirements["requirements"].append(
        {
            "id": "REQ-future-catalog",
            "statement": "A later product area remains intentionally unresolved.",
            "scope": "target",
            "impact": "high",
            "state": "unknown",
            "owner": None,
            "evidenceRefs": [],
            "acceptance": [],
        }
    )
    write_object(requirements_path, requirements)

    contracts_path = bundle / "contracts.json"
    contracts = read_object(contracts_path)
    capability_only_contract = json.loads(
        json.dumps(
            next(
                item
                for item in contracts["contracts"]
                if item["id"] == "advisor-candidate@v1"
            )
        )
    )
    capability_only_contract["id"] = "capability-only@v1"
    capability_only_contract["schemaLocator"] = (
        "inline://contracts.json#/contracts/capability-only@v1"
    )
    contracts["contracts"].append(capability_only_contract)
    stage_b_contract = json.loads(
        json.dumps(
            next(
                item
                for item in contracts["contracts"]
                if item["id"] == "evidence@v1"
            )
        )
    )
    stage_b_contract["id"] = "stage-b-query@v1"
    stage_b_contract["schemaLocator"] = (
        "inline://contracts.json#/contracts/stage-b-query@v1"
    )
    contracts["contracts"].append(stage_b_contract)
    write_object(contracts_path, contracts)

    domains_path = bundle / "domains.json"
    domains = read_object(domains_path)
    domains["domains"][0]["queries"].append("stage-b-query@v1")
    write_object(domains_path, domains)

    entitlements_path = bundle / "entitlements.json"
    entitlements = read_object(entitlements_path)
    capability_only_entitlement = json.loads(
        json.dumps(entitlements["capabilities"][0])
    )
    capability_only_entitlement["id"] = "ENT-capability-only"
    capability_only_entitlement["contractRefs"] = ["capability-only@v1"]
    entitlements["capabilities"].append(capability_only_entitlement)
    write_object(entitlements_path, entitlements)

    scenarios_path = bundle / "ai-scenarios.json"
    scenarios = read_object(scenarios_path)
    scenario = scenarios["scenarios"][0]
    scenario["commercialAccess"] = {"capabilityRef": "ENT-capability-only"}
    scenario["processorRef"] = "processor://self-test"
    scenario["processorRefs"] = ["processor://self-test"]
    scenario["forbiddenVendorRefs"] = ["vendor://self-test-forbidden"]
    scenario["storageProcessorRefs"] = ["processor://self-test-storage"]
    scenario["executionProcessorRefs"] = ["processor://self-test-execution"]
    scenario["benchmarkProcessorRefs"] = ["processor://self-test-benchmark"]
    scenario["forbiddenProcessorRefs"] = ["processor://self-test-forbidden"]
    scenario["retentionPolicyRef"] = "retention-policy.json"
    scenario["commercialBoundaryRef"] = "commercial-boundary.json"
    scenario["qualityPolicyRef"] = "ai-quality-policy.json"
    unselected_ai_consumer = json.loads(json.dumps(scenario))
    unselected_ai_consumer["id"] = "AI-stage-b-consumer"
    unselected_ai_consumer["inputContracts"] = ["evidence@v1"]
    scenarios["scenarios"].append(unselected_ai_consumer)
    write_object(scenarios_path, scenarios)

    release_units_path = bundle / "release-units.json"
    release_units = {
        "schemaVersion": 1,
        "releaseUnits": [
            {
                "id": "RU-contract",
                "status": "review",
                "objective": "Implement the frozen learning command contract.",
                "decisionRef": "DEC-release-unit",
                "dependsOnReleaseUnits": [],
                "roots": {
                    "workPackages": ["WP-contract"],
                    "aiScenarios": ["AI-self-test"],
                },
                "closure": None,
                "deferredSnapshot": None,
                "freeze": None,
            }
        ],
    }
    write_object(release_units_path, release_units)

    review = validate_bundle(root, "review", "RU-contract")
    if review.errors or review.release_unit_context is None:
        raise AssertionError(f"Release-unit review failed: {review.errors}")
    deferred = review.release_unit_context["deferredSnapshot"]["items"]
    if not any(item["subjectId"] == "REQ-future-catalog" for item in deferred):
        raise AssertionError("Out-of-scope unknown was not captured in deferredSnapshot")
    if "learning-create@v1" not in review.release_unit_context["closure"]["contracts"]:
        raise AssertionError("Release-unit closure omitted a work-package output contract")
    if "ENT-capability-only" not in review.release_unit_context["closure"]["entitlements"]:
        raise AssertionError("capabilityRef did not close to its entitlement")
    if "capability-only@v1" not in review.release_unit_context["closure"]["contracts"]:
        raise AssertionError("Capability-selected entitlement dependencies were not closed")
    if "stage-b-query@v1" in review.release_unit_context["closure"]["contracts"]:
        raise AssertionError(
            "A passively included broad domain pulled an unrelated later-stage contract"
        )
    if "AI-stage-b-consumer" in review.release_unit_context["closure"]["aiScenarios"]:
        raise AssertionError(
            "A common input contract reverse-imported an unselected AI consumer"
        )

    catalog_manifest = read_object(manifest_path)
    catalog_artifacts = {
        key: read_object(bundle / relative)
        for key, relative in catalog_manifest["artifacts"].items()
    }
    explicit_domain_unit = json.loads(
        json.dumps(release_units["releaseUnits"][0])
    )
    explicit_domain_unit["id"] = "RU-domain-root"
    explicit_domain_unit["roots"]["domains"] = ["learning"]
    explicit_domain_closure = compute_release_unit_closure(
        catalog_manifest, catalog_artifacts, explicit_domain_unit
    )
    if "stage-b-query@v1" not in explicit_domain_closure["contracts"]:
        raise AssertionError("An explicit domain root did not close its complete contract catalog")

    passive_payload = project_release_unit_payload(
        catalog_manifest,
        catalog_artifacts,
        release_units["releaseUnits"][0],
        review.release_unit_context["closure"],
        review.release_unit_context["deferredSnapshot"],
        commit="self-test",
        frozen_at="2026-01-01T00:00:00+00:00",
    )
    passive_domain = next(
        item
        for item in passive_payload["artifacts"]["domains"]["domains"]
        if item["id"] == "learning"
    )
    if any(
        field in passive_domain
        for field in ("owns", "commands", "queries", "emits", "consumes")
    ):
        raise AssertionError("A passive domain snapshot retained unrelated catalog edges")
    explicit_payload = project_release_unit_payload(
        catalog_manifest,
        catalog_artifacts,
        explicit_domain_unit,
        explicit_domain_closure,
        review.release_unit_context["deferredSnapshot"],
        commit="self-test",
        frozen_at="2026-01-01T00:00:00+00:00",
    )
    explicit_domain = next(
        item
        for item in explicit_payload["artifacts"]["domains"]["domains"]
        if item["id"] == "learning"
    )
    if "stage-b-query@v1" not in explicit_domain.get("queries", []):
        raise AssertionError("An explicit domain root snapshot was incorrectly narrowed")

    scenarios = read_object(scenarios_path)
    scenarios["scenarios"][0]["unregisteredRegistryRef"] = "registry://unsupported"
    write_object(scenarios_path, scenarios)
    assert_error(
        root,
        "review",
        "uses unsupported reference field unregisteredRegistryRef",
        release_unit_id="RU-contract",
    )
    scenarios["scenarios"][0].pop("unregisteredRegistryRef")
    write_object(scenarios_path, scenarios)

    packages_path = bundle / "work-packages.json"
    packages = read_object(packages_path)
    packages["packages"][0]["futurePolicyRef"] = "POLICY-unregistered"
    write_object(packages_path, packages)
    assert_error(
        root,
        "review",
        "uses unsupported reference field futurePolicyRef",
        release_unit_id="RU-contract",
    )
    packages["packages"][0].pop("futurePolicyRef")
    write_object(packages_path, packages)

    contracts_path = bundle / "contracts.json"
    contracts = read_object(contracts_path)
    command = next(
        item
        for item in contracts["contracts"]
        if item["id"] == "learning-create@v1"
    )
    command["status"] = "draft"
    write_object(contracts_path, contracts)
    assert_error(root, "review", "Contract remains draft", release_unit_id="RU-contract")
    command["status"] = "specified"
    write_object(contracts_path, contracts)

    scripts = Path(__file__).resolve().parent
    seal = subprocess.run(
        [
            sys.executable,
            str(scripts / "seal_governance.py"),
            "--project-root",
            str(root),
            "--decision-id",
            "DEC-release-unit",
            "--release-unit",
            "RU-contract",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if seal.returncode != 0:
        raise AssertionError(f"Release-unit sealing failed: {seal.stdout}\n{seal.stderr}")
    if read_object(manifest_path)["status"] != "draft":
        raise AssertionError("Release-unit sealing falsely froze the full catalog")
    assert_valid(root, "freeze", release_unit_id="RU-contract")

    dispatch = subprocess.run(
        [
            sys.executable,
            str(scripts / "dispatch_work_package.py"),
            "--project-root",
            str(root),
            "--package-id",
            "WP-contract",
            "--assigned-to",
            "release-unit-self-test-agent",
            "--release-unit",
            "RU-contract",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if dispatch.returncode != 0:
        raise AssertionError(
            f"Release-unit dispatch failed: {dispatch.stdout}\n{dispatch.stderr}"
        )
    run_id = dispatch.stdout.strip()
    implementation_path = root / "contracts" / "release-unit-implementation.txt"
    implementation_path.parent.mkdir(parents=True, exist_ok=True)
    implementation_path.write_text(
        "implemented from a frozen release unit\n", encoding="utf-8"
    )
    verify = subprocess.run(
        [
            sys.executable,
            str(scripts / "verify_work_package.py"),
            "--project-root",
            str(root),
            "--run-id",
            run_id,
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if verify.returncode != 0:
        raise AssertionError(
            f"Release-unit verification failed: {verify.stdout}\n{verify.stderr}"
        )
    registry = read_object(bundle / "execution" / "work-package-runs.json")
    run = next(item for item in registry["runs"] if item["id"] == run_id)
    if "manifestHash" in run or not run.get("releaseUnitHash"):
        raise AssertionError("Release-unit run used the global manifest identity")
    report = read_object(bundle / run["reportPath"])
    if (
        report.get("releaseUnitId") != "RU-contract"
        or report.get("releaseUnitHash") != run.get("releaseUnitHash")
        or "manifestHash" in report
    ):
        raise AssertionError("Release-unit report identity is not exact")
    release_owned_hashes = report.get("testedOwnedState", {}).get("pathHashes", {})
    if "tests/work_packages/wp-self/test_acceptance.py" not in release_owned_hashes:
        raise AssertionError(
            "Release-unit report omitted a baseline-existing owned test file"
        )
    assert_valid(root, "freeze", release_unit_id="RU-contract")

    test_path = root / "tests" / "work_packages" / "wp-self" / "test_acceptance.py"
    release_test_source = test_path.read_text(encoding="utf-8")
    test_path.write_text(
        release_test_source + "\n# release-unit owned-state drift\n",
        encoding="utf-8",
    )
    assert_error(
        root,
        "freeze",
        "completed tested owned state changed after verification: tests/work_packages/wp-self/test_acceptance.py",
        release_unit_id="RU-contract",
    )
    test_path.write_text(release_test_source, encoding="utf-8")
    assert_valid(root, "freeze", release_unit_id="RU-contract")

    packages = read_object(packages_path)
    consumer_package = json.loads(json.dumps(packages["packages"][0]))
    consumer_package.update(
        {
            "id": "WP-consumer",
            "wave": "consumer",
            "objective": "Consume the contract completed by a dependency release unit.",
            "deliverables": ["A dependency-bound consumer fixture."],
            "dependsOn": ["WP-contract"],
            "allowedPaths": [
                "consumer/**",
                "tests/work_packages/wp-consumer/**",
            ],
            "inputContracts": ["learning-create@v1"],
            "outputContracts": [],
            "acceptanceCommands": [
                {
                    "id": "wp-consumer-as-01-execute",
                    "kind": "scenario_test",
                    "adapter": "pytest-json@1",
                    "selector": "tests/work_packages/wp-consumer/test_acceptance.py::test_wp_consumer_as_01",
                    "cwd": ".",
                    "timeoutSeconds": 30,
                }
            ],
            "requiredAcceptanceScenarios": [
                {
                    "id": "WP-consumer-AS-01",
                    "assertion": "the downstream consumer observes the completed upstream artifact under its own release-unit identity",
                    "testSelector": "tests/work_packages/wp-consumer/test_acceptance.py::test_wp_consumer_as_01",
                    "acceptanceCommandId": "wp-consumer-as-01-execute",
                }
            ],
        }
    )
    consumer_package.pop("outputArtifacts", None)
    packages["packages"].append(consumer_package)
    write_object(packages_path, packages)
    consumer_test = (
        root / "tests" / "work_packages" / "wp-consumer" / "test_acceptance.py"
    )
    consumer_test.parent.mkdir(parents=True, exist_ok=True)
    consumer_test.write_text(
        "import os\n"
        "from pathlib import Path\n\n"
        "def test_wp_consumer_as_01():\n"
        "    assert os.environ['GOVERNANCE_RELEASE_UNIT_ID'] == 'RU-consumer'\n"
        "    root = Path(os.environ['GOVERNANCE_PROJECT_ROOT'])\n"
        "    assert (root / 'contracts' / 'release-unit-implementation.txt').is_file()\n"
        "    assert (root / 'consumer' / 'implementation.txt').is_file()\n",
        encoding="utf-8",
    )

    release_units = read_object(release_units_path)
    release_units["releaseUnits"].append(
        {
            "id": "RU-consumer",
            "status": "review",
            "objective": "Consume a package completed by the frozen contract unit.",
            "decisionRef": "DEC-release-unit",
            "dependsOnReleaseUnits": ["RU-contract"],
            "roots": {"workPackages": ["WP-consumer"]},
            "closure": None,
            "deferredSnapshot": None,
            "freeze": None,
        }
    )
    write_object(release_units_path, release_units)
    consumer_review = validate_bundle(root, "review", "RU-consumer")
    if consumer_review.errors or consumer_review.release_unit_context is None:
        raise AssertionError(
            f"Dependent release-unit review failed: {consumer_review.errors}"
        )
    consumer_closure = consumer_review.release_unit_context["closure"]["workPackages"]
    if consumer_closure != ["WP-consumer"]:
        raise AssertionError(
            f"Dependent release unit copied externally satisfied packages: {consumer_closure}"
        )

    consumer_seal = subprocess.run(
        [
            sys.executable,
            str(scripts / "seal_governance.py"),
            "--project-root",
            str(root),
            "--decision-id",
            "DEC-release-unit",
            "--release-unit",
            "RU-consumer",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if consumer_seal.returncode != 0:
        raise AssertionError(
            f"Dependent release-unit sealing failed: {consumer_seal.stdout}\n{consumer_seal.stderr}"
        )
    release_units = read_object(release_units_path)
    upstream_unit = next(
        item for item in release_units["releaseUnits"] if item["id"] == "RU-contract"
    )
    upstream_snapshot_path = bundle / upstream_unit["freeze"]["snapshotPath"]
    upstream_snapshot_bytes = upstream_snapshot_path.read_bytes()
    upstream_snapshot = read_object(upstream_snapshot_path)
    upstream_snapshot["objective"] = "tampered before dependent dispatch"
    write_object(upstream_snapshot_path, upstream_snapshot)
    rejected_dispatch = subprocess.run(
        [
            sys.executable,
            str(scripts / "dispatch_work_package.py"),
            "--project-root",
            str(root),
            "--package-id",
            "WP-consumer",
            "--assigned-to",
            "dependency-hash-negative-agent",
            "--release-unit",
            "RU-consumer",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if rejected_dispatch.returncode == 0:
        raise AssertionError("Dependent dispatch accepted a tampered upstream snapshot")
    upstream_snapshot_path.write_bytes(upstream_snapshot_bytes)
    assert_valid(root, "freeze", release_unit_id="RU-consumer")

    consumer_dispatch = subprocess.run(
        [
            sys.executable,
            str(scripts / "dispatch_work_package.py"),
            "--project-root",
            str(root),
            "--package-id",
            "WP-consumer",
            "--assigned-to",
            "release-unit-consumer-agent",
            "--release-unit",
            "RU-consumer",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if consumer_dispatch.returncode != 0:
        raise AssertionError(
            f"Dependent release-unit dispatch failed: {consumer_dispatch.stdout}\n{consumer_dispatch.stderr}"
        )
    consumer_implementation = root / "consumer" / "implementation.txt"
    consumer_implementation.parent.mkdir(parents=True, exist_ok=True)
    consumer_implementation.write_text(
        "implemented after validating the frozen dependency\n", encoding="utf-8"
    )
    consumer_verify = subprocess.run(
        [
            sys.executable,
            str(scripts / "verify_work_package.py"),
            "--project-root",
            str(root),
            "--run-id",
            consumer_dispatch.stdout.strip(),
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if consumer_verify.returncode != 0:
        raise AssertionError(
            f"Dependent release-unit verification failed: {consumer_verify.stdout}\n{consumer_verify.stderr}"
        )
    assert_valid(root, "freeze", release_unit_id="RU-consumer")

    # A successor may take over the exact implementation paths only through an
    # explicit, dependency-bound release-unit supersession. The current catalog
    # retires the old runnable package while the immutable old snapshot remains
    # available for historical verification.
    packages = read_object(packages_path)
    original_package = next(
        item for item in packages["packages"] if item["id"] == "WP-contract"
    )
    original_package["status"] = "retired"
    successor_package = json.loads(json.dumps(original_package))
    successor_package.update(
        {
            "id": "WP-contract-v2",
            "status": "planned",
            "objective": "Correct and supersede the frozen contract implementation authority.",
            "deliverables": ["A successor artifact on the exact canonical paths."],
            # Reconciliation verifies the old report historically, not as a
            # claim that its old current-source execution is still satisfied.
            "dependsOn": [],
            "allowedPaths": [
                "contracts/**",
                "tests/work_packages/wp-successor/**",
            ],
            "inputContracts": ["learning-create@v1"],
            "outputContracts": [],
            "acceptanceCommands": [
                {
                    "id": "wp-contract-v2-as-01-execute",
                    "kind": "scenario_test",
                    "adapter": "pytest-json@1",
                    "selector": "tests/work_packages/wp-successor/test_acceptance.py::test_wp_contract_v2_as_01",
                    "cwd": ".",
                    "timeoutSeconds": 30,
                }
            ],
            "requiredAcceptanceScenarios": [
                {
                    "id": "WP-contract-v2-AS-01",
                    "assertion": "the successor owns the exact canonical paths under its new immutable authority",
                    "testSelector": "tests/work_packages/wp-successor/test_acceptance.py::test_wp_contract_v2_as_01",
                    "acceptanceCommandId": "wp-contract-v2-as-01-execute",
                }
            ],
        }
    )
    successor_package.pop("outputArtifacts", None)
    packages["packages"].append(successor_package)
    write_object(packages_path, packages)
    successor_test = (
        root / "tests" / "work_packages" / "wp-successor" / "test_acceptance.py"
    )
    successor_test.parent.mkdir(parents=True, exist_ok=True)
    successor_test.write_text(
        "import os\n\n"
        "def test_wp_contract_v2_as_01():\n"
        "    assert os.environ['GOVERNANCE_RELEASE_UNIT_ID'] == 'RU-contract-v2'\n",
        encoding="utf-8",
    )

    release_units = read_object(release_units_path)
    release_units["releaseUnits"].append(
        {
            "id": "RU-contract-v2",
            "status": "review",
            "objective": "Supersede one frozen implementation authority without rewriting history.",
            "decisionRef": "DEC-release-unit",
            "dependsOnReleaseUnits": ["RU-contract"],
            "supersedesReleaseUnits": ["RU-contract"],
            "roots": {"workPackages": ["WP-contract-v2"]},
            "closure": None,
            "deferredSnapshot": None,
            "freeze": None,
        }
    )
    write_object(release_units_path, release_units)
    # Regression: acknowledged drift must not deadlock the successor review,
    # but neither a generic accepted decision nor a planned unit is a waiver.
    from baseline_reconciliation import capture_state, reconciled_run_ids
    from release_units import release_unit_index
    from execution_integrity import sha256 as file_sha256
    unit_id = "RU-contract-v2"
    original_source = implementation_path.read_bytes()
    implementation_path.write_text("explicitly acknowledged pre-freeze drift\n", encoding="utf-8")
    assert_error(root, "review", "completed tested owned state changed", release_unit_id=unit_id)
    units_index = release_unit_index(read_object(release_units_path))
    unit = units_index[unit_id]
    decisions_path = bundle / "decisions.json"
    decisions = read_object(decisions_path)
    try:
        capture_state(root, unit, units_index, decisions["decisions"])
    except ValueError as exc:
        if "explicit accepted decision" not in str(exc):
            raise
    else:
        raise AssertionError("Generic acceptance bypassed explicit reconciliation consent")
    decision = next(d for d in decisions["decisions"] if d["id"] == unit["decisionRef"])
    decision["approvesBaselineReconciliation"] = True
    write_object(decisions_path, decisions)
    packages_before_scope_test = packages_path.read_bytes()
    narrow_packages = read_object(packages_path)
    narrow = next(p for p in narrow_packages["packages"] if p["id"] == "WP-contract-v2")
    narrow["allowedPaths"] = ["tests/work_packages/wp-successor/**"]
    write_object(packages_path, narrow_packages)
    try:
        capture_state(root, unit, units_index, decisions["decisions"])
    except ValueError as exc:
        if "cannot transfer unowned drift" not in str(exc):
            raise
    else:
        raise AssertionError("A narrow successor excused unrelated predecessor drift")
    packages_path.write_bytes(packages_before_scope_test)
    receipt = capture_state(root, unit, units_index, decisions["decisions"])
    import hashlib
    receipt_bytes = (json.dumps(receipt, ensure_ascii=False, indent=2) + "\n").encode()
    receipt_hash = "sha256:" + hashlib.sha256(receipt_bytes).hexdigest()
    receipt_relative = f"execution/baseline-reconciliations/{receipt_hash[7:]}.json"
    receipt_path = bundle / receipt_relative
    write_object(receipt_path, receipt)
    unit["baselineReconciliation"] = {"receiptPath": receipt_relative, "receiptSha256": receipt_hash}
    release_units = read_object(release_units_path)
    release_units["releaseUnits"][-1] = unit
    write_object(release_units_path, release_units)
    assert_valid(root, "review", release_unit_id=unit_id)
    # Public/global checks must not inherit a review unit's acknowledgement.
    assert_error(root, "draft", "completed tested owned state changed")
    implementation_path.write_text("not acknowledged later drift\n", encoding="utf-8")
    assert_error(root, "review", "Baseline reconciliation is stale", release_unit_id=unit_id)
    implementation_path.write_text("explicitly acknowledged pre-freeze drift\n", encoding="utf-8")
    receipt_path.write_text("{}", encoding="utf-8")
    assert_error(root, "review", "receipt is missing or changed", release_unit_id=unit_id)
    receipt_path.write_bytes(receipt_bytes)
    proof_path = bundle / receipt["runs"][0]["reportPath"]
    proof_bytes = proof_path.read_bytes()
    proof_path.write_text("{}", encoding="utf-8")
    assert_error(root, "review", "cannot excuse invalid historical proof", release_unit_id=unit_id)
    proof_path.write_bytes(proof_bytes)
    unrelated_bytes = consumer_implementation.read_bytes()
    consumer_implementation.write_text("unrelated unit drift\n", encoding="utf-8")
    assert_error(root, "review", "completed tested owned state changed", release_unit_id=unit_id)
    consumer_implementation.write_bytes(unrelated_bytes)
    registry_path = bundle / "execution/work-package-runs.json"
    registry_bytes = registry_path.read_bytes()
    registry = read_object(registry_path)
    registry["runs"].append({"id": "pending", "status": "in_progress"})
    write_object(registry_path, registry)
    assert_error(root, "review", "cannot replace an active run", release_unit_id=unit_id)
    registry_path.write_bytes(registry_bytes)
    if file_sha256(proof_path) != receipt["runs"][0]["reportSha256"]:
        raise AssertionError("Historical proof changed during reconciliation")
    assert_valid(root, "review", release_unit_id=unit_id)
    successor_seal = subprocess.run(
        [
            sys.executable,
            str(scripts / "seal_governance.py"),
            "--project-root",
            str(root),
            "--decision-id",
            "DEC-release-unit",
            "--release-unit",
            "RU-contract-v2",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if successor_seal.returncode != 0:
        raise AssertionError(
            f"Successor release-unit sealing failed: {successor_seal.stdout}\n{successor_seal.stderr}"
        )
    assert_valid(root, "freeze", release_unit_id="RU-contract-v2")
    assert_valid(root, "freeze", release_unit_id="RU-contract")
    frozen_catalog_bytes = release_units_path.read_bytes()
    tampered_catalog = read_object(release_units_path)
    tampered_unit = next(u for u in tampered_catalog["releaseUnits"] if u["id"] == unit_id)
    tampered_unit["objective"] = "changed without a new immutable authority"
    write_object(release_units_path, tampered_catalog)
    assert_error(root, "freeze", "frozen authority objective mismatch", release_unit_id=unit_id)
    release_units_path.write_bytes(frozen_catalog_bytes)
    registry_bytes = registry_path.read_bytes()
    truncated_registry = read_object(registry_path)
    truncated_registry["runs"] = [r for r in truncated_registry["runs"] if r["id"] != receipt["runs"][0]["runId"]]
    write_object(registry_path, truncated_registry)
    assert_error(root, "freeze", "lost or changed its referenced run", release_unit_id=unit_id)
    registry_path.write_bytes(registry_bytes)
    dispatch_argv = [sys.executable, str(scripts / "dispatch_work_package.py"),
                     "--project-root", str(root), "--release-unit", unit_id,
                     "--package-id", "WP-contract-v2", "--assigned-to", "reconciled-successor"]
    implementation_path.write_text("unacknowledged change between freeze and dispatch\n", encoding="utf-8")
    stale_dispatch = subprocess.run(dispatch_argv, capture_output=True, text=True)
    if stale_dispatch.returncode == 0 or "Source changed after baseline reconciliation" not in stale_dispatch.stderr:
        raise AssertionError("Dispatch accepted unacknowledged post-freeze source drift: " + stale_dispatch.stderr)
    implementation_path.write_text("explicitly acknowledged pre-freeze drift\n", encoding="utf-8")
    successor_dispatch = subprocess.run(dispatch_argv, capture_output=True, text=True)
    if successor_dispatch.returncode != 0:
        raise AssertionError("Reconciled successor could not dispatch: " + successor_dispatch.stderr)
    implementation_path.write_text(
        "replaced only after the successor authority was frozen\n", encoding="utf-8"
    )
    assert_valid(root, "freeze", release_unit_id="RU-contract")
    assert_valid(root, "freeze", release_unit_id="RU-contract-v2")
    successor_verify = subprocess.run(
        [sys.executable, str(scripts / "verify_work_package.py"), "--project-root", str(root),
         "--run-id", successor_dispatch.stdout.strip()], capture_output=True, text=True,
    )
    if successor_verify.returncode != 0:
        raise AssertionError("Reconciled successor lacked fresh verification: " + successor_verify.stderr)
    rejected_superseded_dispatch = subprocess.run(
        [
            sys.executable,
            str(scripts / "dispatch_work_package.py"),
            "--project-root",
            str(root),
            "--package-id",
            "WP-contract",
            "--assigned-to",
            "must-not-run",
            "--release-unit",
            "RU-contract",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if rejected_superseded_dispatch.returncode == 0 or "is superseded by" not in (
        rejected_superseded_dispatch.stdout + rejected_superseded_dispatch.stderr
    ):
        raise AssertionError("A superseded release unit accepted a new dispatch")

    release_units = read_object(release_units_path)
    invalid_supersession = {
        "id": "RU-invalid-supersession",
        "status": "review",
        "objective": "Prove supersession cannot bypass dependency identity.",
        "decisionRef": "DEC-release-unit",
        "dependsOnReleaseUnits": [],
        "supersedesReleaseUnits": ["RU-contract"],
        "roots": {"workPackages": ["WP-contract-v2"]},
        "closure": None,
        "deferredSnapshot": None,
        "freeze": None,
    }
    release_units["releaseUnits"].append(invalid_supersession)
    write_object(release_units_path, release_units)
    assert_error(
        root,
        "review",
        "supersedesReleaseUnits must be a subset of dependsOnReleaseUnits",
        release_unit_id="RU-invalid-supersession",
    )
    release_units["releaseUnits"].pop()
    competing_superseder = json.loads(json.dumps(invalid_supersession))
    competing_superseder["id"] = "RU-competing-superseder"
    competing_superseder["dependsOnReleaseUnits"] = ["RU-contract"]
    release_units["releaseUnits"].append(competing_superseder)
    write_object(release_units_path, release_units)
    assert_error(
        root,
        "review",
        "has multiple direct superseders",
        release_unit_id="RU-competing-superseder",
    )
    release_units["releaseUnits"].pop()
    write_object(release_units_path, release_units)

    release_units = read_object(release_units_path)
    release_units["releaseUnits"].append(
        {
            "id": "RU-overlap",
            "status": "review",
            "objective": "Prove cross-unit ownership conflicts remain blocking.",
            "decisionRef": "DEC-release-unit",
            "dependsOnReleaseUnits": [],
            "roots": {"workPackages": ["WP-contract"]},
            "closure": None,
            "deferredSnapshot": None,
            "freeze": None,
        }
    )
    write_object(release_units_path, release_units)
    assert_error(
        root,
        "review",
        "Cross-release-unit allowedPaths overlap",
        release_unit_id="RU-overlap",
    )

    frozen = release_units["releaseUnits"][0]
    snapshot_path = bundle / frozen["freeze"]["snapshotPath"]
    snapshot = read_object(snapshot_path)
    snapshot["objective"] = "tampered release-unit snapshot"
    write_object(snapshot_path, snapshot)
    assert_error(
        root,
        "freeze",
        "snapshot hash mismatch",
        release_unit_id="RU-contract",
    )


def make_legacy_v1_bundle(root: Path) -> None:
    bundle = root / BUNDLE_DIR
    manifest_path = bundle / "manifest.json"
    manifest = read_object(manifest_path)
    release_path = manifest["artifacts"].pop("releaseUnits")
    write_object(manifest_path, manifest)
    (bundle / release_path).unlink()


def exercise_sequential_output_handoff(root: Path, release_unit: bool) -> None:
    """Ordered implementation attempts preserve proofs and protect current outputs."""
    bundle = root / BUNDLE_DIR
    packages = read_object(bundle / "work-packages.json")
    implementation = packages["packages"][0]
    baseline = json.loads(json.dumps(implementation))
    baseline.update(id="WP-baseline", verificationOnly=True, outputContracts=[])
    baseline.pop("outputArtifacts", None)
    implementation["dependsOn"] = ["WP-baseline"]
    implementation["allowedPaths"] = ["contracts/edit/**", "tests/work_packages/wp-self/**"]
    implementation["forbiddenPaths"].append("contracts/protected/**")
    implementation.pop("outputArtifacts", None)
    verification = packages["packages"][1]
    verification["dependsOn"] = ["WP-contract"]
    verification["allowedPaths"].insert(0, "contracts/edit/**")
    packages["packages"].insert(0, baseline)
    write_object(bundle / "work-packages.json", packages)
    edited = root / "contracts/edit/output.txt"
    kept = root / "contracts/keep.txt"
    forbidden = root / "contracts/protected/keep.txt"
    for path in (edited, kept, forbidden):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("baseline\n", encoding="utf-8")
    acceptance_test = (
        root / "tests/work_packages/wp-self/test_acceptance.py"
    )
    acceptance_test.write_text(
        acceptance_test.read_text(encoding="utf-8")
        + "    from pathlib import Path\n"
        + "    assert Path('contracts/edit/output.txt').read_text(encoding='utf-8') != 'intentionally failing implementation\\n'\n",
        encoding="utf-8",
    )
    scripts = Path(__file__).resolve().parent
    unit_args = ["--release-unit", "RU-handoff"] if release_unit else []
    if release_unit:
        units = read_object(bundle / "release-units.json")
        units["releaseUnits"] = [{
            "id": "RU-handoff", "status": "review",
            "objective": "Verify ordered source ownership handoff.",
            "decisionRef": "DEC-freeze", "dependsOnReleaseUnits": [],
            "roots": {"workPackages": ["WP-verify-only"]},
            "closure": None, "deferredSnapshot": None, "freeze": None,
        }]
        write_object(bundle / "release-units.json", units)

    def command(
        script: str, *args: str, ok: bool = True,
        error_contains: str | None = None,
    ) -> str:
        result = subprocess.run(
            [sys.executable, str(scripts / script), "--project-root", str(root), *args],
            check=False, capture_output=True, text=True,
        )
        if (result.returncode == 0) != ok:
            raise AssertionError(f"Unexpected {script} outcome: {result.stdout}\n{result.stderr}")
        if error_contains is not None and error_contains not in (
            result.stdout + result.stderr
        ):
            raise AssertionError(
                f"Expected {script} error containing {error_contains!r}: "
                f"{result.stdout}\n{result.stderr}"
            )
        return result.stdout.strip()

    historical_proofs: dict[str, str] = {}
    historical_runs: dict[str, dict[str, Any]] = {}

    def assert_historical_proofs_unchanged() -> None:
        registry = read_object(bundle / "execution/work-package-runs.json")
        current_runs = {item["id"]: item for item in registry["runs"]}
        for run_id, expected_run in historical_runs.items():
            if current_runs.get(run_id) != expected_run:
                raise AssertionError(
                    f"Sequential dispatch or verification changed a finished run record: {run_id}"
                )
        for relative, expected_hash in historical_proofs.items():
            path = bundle_execution_path(root, relative)
            if not path.is_file() or sha256(path) != expected_hash:
                raise AssertionError(
                    f"Sequential dispatch or verification changed a historical proof: {relative}"
                )

    def remember_finished_proofs(run_id: str, expected_status: str) -> dict[str, Any]:
        assert_historical_proofs_unchanged()
        registry = read_object(bundle / "execution/work-package-runs.json")
        run = next(item for item in registry["runs"] if item["id"] == run_id)
        if run["status"] != expected_status:
            raise AssertionError(
                f"Expected {run_id} to be {expected_status}, got {run['status']}"
            )
        historical_runs[run_id] = json.loads(json.dumps(run))
        report = read_object(bundle_execution_path(root, run["reportPath"]))
        relatives = [
            run["baselineSnapshotPath"], run["reportPath"], report["testedSnapshotPath"],
            *(item["path"] for item in report["scenarioEvidence"]),
        ]
        for relative in relatives:
            historical_proofs[relative] = sha256(bundle_execution_path(root, relative))
        return report

    def assert_latest_owned_outputs(run_id: str, expected_text: str) -> None:
        edited.write_text("drift after latest verification\n", encoding="utf-8")
        assert_error(
            root, "freeze",
            f"Execution run {run_id} integrity failure: completed tested owned state "
            "changed after verification: contracts/edit/output.txt",
            **validation_args,
        )
        edited.write_text(expected_text, encoding="utf-8")
        assert_valid(root, "freeze", **validation_args)
        assert_historical_proofs_unchanged()

    def fail_implementation_attempt(run_id: str) -> None:
        edited.write_text("intentionally failing implementation\n", encoding="utf-8")
        assert_valid(root, "freeze", **validation_args)
        command("verify_work_package.py", "--run-id", run_id, ok=False)
        report = remember_finished_proofs(run_id, "failed")
        if report["sourceScopeFailures"] or not any(
            item.get("actualExitCode") not in (None, 0) for item in report["commands"]
        ):
            raise AssertionError("Retry fixture did not fail through its selected acceptance test")
        assert_valid(root, "freeze", **validation_args)

    command("seal_governance.py", "--decision-id", "DEC-freeze", *unit_args)
    baseline_id = command("dispatch_work_package.py", "--package-id", "WP-baseline",
                          "--assigned-to", "baseline-auditor", *unit_args)
    command("verify_work_package.py", "--run-id", baseline_id)
    baseline_registry = read_object(bundle / "execution/work-package-runs.json")
    baseline_run = baseline_registry["runs"][-1]
    baseline_report_path = bundle / baseline_run["reportPath"]
    baseline_report_bytes = baseline_report_path.read_bytes()
    validation_args = {"release_unit_id": "RU-handoff"} if release_unit else {}
    remember_finished_proofs(baseline_id, "completed")

    # A planned downstream package is not permission for pre-dispatch drift.
    edited.write_text("changed before dispatch\n", encoding="utf-8")
    assert_error(root, "freeze", "changed after verification", **validation_args)
    command("dispatch_work_package.py", "--package-id", "WP-contract",
            "--assigned-to", "too-early", *unit_args, ok=False)
    edited.write_text("baseline\n", encoding="utf-8")
    implementation_id = command("dispatch_work_package.py", "--package-id", "WP-contract",
                                "--assigned-to", "implementation-owner", *unit_args)
    edited.write_text("governed implementation\n", encoding="utf-8")
    assert_valid(root, "freeze", **validation_args)
    for path in (kept, forbidden):
        path.write_text("not handed over\n", encoding="utf-8")
        assert_error(root, "freeze", f"changed after verification: {path.relative_to(root)}",
                     **validation_args)
        path.write_text("baseline\n", encoding="utf-8")

    # Historical proof remains mandatory while a descendant is writing source.
    baseline_report_path.write_bytes(baseline_report_bytes + b"\n")
    assert_error(root, "freeze", "report hash mismatch", **validation_args)
    baseline_report_path.write_bytes(baseline_report_bytes)
    fail_implementation_attempt(implementation_id)
    command(
        "dispatch_work_package.py", "--package-id", "WP-verify-only",
        "--assigned-to", "failed-dependency-consumer", *unit_args, ok=False,
        error_contains="Dependency WP-contract has no completed run",
    )
    implementation_id = command(
        "dispatch_work_package.py", "--package-id", "WP-contract",
        "--assigned-to", "initial-retry-owner", *unit_args,
    )
    edited.write_text("governed implementation\n", encoding="utf-8")
    command("verify_work_package.py", "--run-id", implementation_id)
    remember_finished_proofs(implementation_id, "completed")
    assert_valid(root, "freeze", **validation_args)
    assert_latest_owned_outputs(implementation_id, "governed implementation\n")

    # A completed read-only descendant cannot freeze the implementation ancestor's
    # next authorized attempt. Its report and evidence remain historical proof.
    verification_id = command(
        "dispatch_work_package.py", "--package-id", "WP-verify-only",
        "--assigned-to", "read-only-descendant", *unit_args,
    )
    command("verify_work_package.py", "--run-id", verification_id)
    remember_finished_proofs(verification_id, "completed")
    retry_id = command(
        "dispatch_work_package.py", "--package-id", "WP-contract",
        "--assigned-to", "retry-after-read-only-owner", *unit_args,
    )
    edited.write_text("implementation after read-only verification\n", encoding="utf-8")
    assert_valid(root, "freeze", **validation_args)
    command("verify_work_package.py", "--run-id", retry_id)
    remember_finished_proofs(retry_id, "completed")
    assert_latest_owned_outputs(retry_id, "implementation after read-only verification\n")

    # Failing after an earlier successful attempt must not force source rollback
    # merely to dispatch the next retry. The failed attempt remains uncompleted.
    failing_retry_id = command(
        "dispatch_work_package.py", "--package-id", "WP-contract",
        "--assigned-to", "failing-retry-owner", *unit_args,
    )
    fail_implementation_attempt(failing_retry_id)
    command(
        "dispatch_work_package.py", "--package-id", "WP-verify-only",
        "--assigned-to", "failed-retry-dependency-consumer", *unit_args, ok=False,
        error_contains="Dependency WP-contract",
    )
    final_retry_id = command(
        "dispatch_work_package.py", "--package-id", "WP-contract",
        "--assigned-to", "retry-after-failure-owner", *unit_args,
    )
    edited.write_text("implementation after failed retry\n", encoding="utf-8")
    assert_valid(root, "freeze", **validation_args)
    command("verify_work_package.py", "--run-id", final_retry_id)
    remember_finished_proofs(final_retry_id, "completed")
    assert_latest_owned_outputs(final_retry_id, "implementation after failed retry\n")
    assert_valid(root, "freeze", **validation_args)
    assert_historical_proofs_unchanged()
    if baseline_report_path.read_bytes() != baseline_report_bytes:
        raise AssertionError("Sequential implementation rewrote the baseline proof")


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="govern-product-build-") as temporary:
        root = Path(temporary)
        initialize_git(root)
        write_fixture(root)
        exercise_pytest_root_rebasing(root)
        make_legacy_v1_bundle(root)
        assert_valid(root, "review")
        exercise_negative_gates(root)
        assert_valid(root, "review")
        seal_and_detect_tampering(root)
    with tempfile.TemporaryDirectory(prefix="govern-product-release-unit-") as temporary:
        root = Path(temporary)
        initialize_git(root)
        write_fixture(root)
        exercise_release_unit_flow(root)
    for release_unit in (False, True):
        with tempfile.TemporaryDirectory(prefix="govern-handoff-") as temporary:
            root = Path(temporary)
            initialize_git(root)
            write_fixture(root)
            exercise_sequential_output_handoff(root, release_unit)
    print("govern-product-build self-test passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
