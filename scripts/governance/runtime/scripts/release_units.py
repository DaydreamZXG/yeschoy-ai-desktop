"""Deterministic implementation release-unit closure and snapshot helpers."""

from __future__ import annotations

from collections import deque
import json
import hashlib
from pathlib import Path, PurePosixPath
import re
from typing import Any, Iterable

from execution_integrity import bundle_execution_path
from governance_common import BUNDLE_DIR, canonical_bytes, governance_hash, read_object


RELEASE_UNIT_ID = re.compile(r"^[A-Za-z][A-Za-z0-9._:-]{1,119}$")
ROOT_KEYS = (
    "workPackages",
    "requirements",
    "nonFunctionalRequirements",
    "domains",
    "contracts",
    "stateMachines",
    "pages",
    "journeys",
    "dataLifecycleResources",
    "aiScenarios",
    "entitlements",
    "trackCoverage",
)
CLOSURE_KEYS = (
    "workPackages",
    "requirements",
    "nonFunctionalRequirements",
    "domains",
    "contracts",
    "stateMachines",
    "pages",
    "pageStateProfiles",
    "journeys",
    "actors",
    "dataLifecycleResources",
    "aiScenarios",
    "entitlements",
    "trackCoverage",
    "evidence",
    "decisions",
    "conflicts",
    "glossaryTerms",
    "resources",
    "tracks",
)


def release_unit_snapshot_relative(release_unit_hash: str) -> str:
    if not re.fullmatch(r"sha256:[0-9a-f]{64}", release_unit_hash):
        raise ValueError("releaseUnitHash must be a sha256 digest")
    return (
        "execution/release-unit-snapshots/"
        f"{release_unit_hash.removeprefix('sha256:')}/release-unit.json"
    )


def release_unit_hash(payload: dict[str, Any]) -> str:
    normalized = json.loads(json.dumps(payload))
    normalized["releaseUnitHash"] = None
    return f"sha256:{hashlib.sha256(canonical_bytes(normalized)).hexdigest()}"


def _entries(
    artifacts: dict[str, dict[str, Any]], artifact: str, collection: str, key: str
) -> dict[str, dict[str, Any]]:
    data = artifacts.get(artifact)
    if not isinstance(data, dict):
        raise ValueError(f"Release units require artifact {artifact}")
    values = data.get(collection)
    if not isinstance(values, list):
        raise ValueError(f"{artifact}.{collection} must be an array")
    result: dict[str, dict[str, Any]] = {}
    for index, item in enumerate(values):
        if not isinstance(item, dict):
            raise ValueError(f"{artifact}.{collection}[{index}] must be an object")
        identifier = item.get(key)
        if not isinstance(identifier, str) or not identifier:
            raise ValueError(f"{artifact}.{collection}[{index}].{key} is required")
        if identifier in result:
            raise ValueError(f"Duplicate {artifact} entry: {identifier}")
        result[identifier] = item
    return result


def artifact_indexes(
    manifest: dict[str, Any], artifacts: dict[str, dict[str, Any]]
) -> dict[str, dict[str, Any]]:
    pages = artifacts["pageMatrix"]
    journeys = artifacts["journeys"]
    coverage_records: dict[str, dict[str, Any]] = {}
    for capability in artifacts["trackCoverage"].get("capabilities", []):
        if not isinstance(capability, dict) or not isinstance(capability.get("capability"), str):
            raise ValueError("trackCoverage.capabilities contains an invalid entry")
        tracks = capability.get("tracks")
        if not isinstance(tracks, dict):
            raise ValueError(
                f"Track coverage {capability['capability']}.tracks must be an object"
            )
        for track, item in tracks.items():
            if not isinstance(track, str) or not isinstance(item, dict):
                raise ValueError(
                    f"Track coverage {capability['capability']} contains an invalid track"
                )
            identifier = f"{capability['capability']}::{track}"
            coverage_records[identifier] = {
                "capability": capability["capability"],
                "track": track,
                "item": item,
            }
    return {
        "workPackages": _entries(artifacts, "workPackages", "packages", "id"),
        "requirements": _entries(artifacts, "requirements", "requirements", "id"),
        "nonFunctionalRequirements": _entries(
            artifacts, "nonFunctionalRequirements", "requirements", "id"
        ),
        "domains": _entries(artifacts, "domains", "domains", "id"),
        "contracts": _entries(artifacts, "contracts", "contracts", "id"),
        "stateMachines": _entries(artifacts, "stateMachines", "machines", "id"),
        "pages": {
            str(item["id"]): item
            for item in pages.get("pages", [])
            if isinstance(item, dict) and isinstance(item.get("id"), str)
        },
        "pageStateProfiles": {
            str(item["id"]): item
            for item in pages.get("stateProfiles", [])
            if isinstance(item, dict) and isinstance(item.get("id"), str)
        },
        "journeys": {
            str(item["id"]): item
            for item in journeys.get("journeys", [])
            if isinstance(item, dict) and isinstance(item.get("id"), str)
        },
        "actors": {
            str(item["id"]): item
            for item in journeys.get("actors", [])
            if isinstance(item, dict) and isinstance(item.get("id"), str)
        },
        "dataLifecycleResources": _entries(
            artifacts, "dataLifecycle", "resources", "id"
        ),
        "aiScenarios": _entries(artifacts, "aiScenarios", "scenarios", "id"),
        "entitlements": _entries(artifacts, "entitlements", "capabilities", "id"),
        "trackCoverage": coverage_records,
        "evidence": _entries(artifacts, "evidenceLedger", "entries", "id"),
        "decisions": _entries(artifacts, "decisionRegister", "decisions", "id"),
        "conflicts": _entries(artifacts, "conflictRegister", "conflicts", "id"),
        "glossaryTerms": _entries(artifacts, "glossary", "terms", "id"),
        "tracks": {str(track): {"id": track} for track in manifest.get("tracks", [])},
    }


def validate_release_unit_shape(unit: Any) -> list[str]:
    errors: list[str] = []
    if not isinstance(unit, dict):
        return ["Release unit must be an object"]
    allowed = {
        "id",
        "status",
        "objective",
        "decisionRef",
        "dependsOnReleaseUnits",
        "supersedesReleaseUnits",
        "roots",
        "closure",
        "deferredSnapshot",
        "freeze",
        "baselineReconciliation",
    }
    extra = set(unit) - allowed
    if extra:
        errors.append(f"Release unit has unsupported fields: {sorted(extra)}")
    reconciliation = unit.get("baselineReconciliation")
    if reconciliation is not None and (
        not isinstance(reconciliation, dict)
        or set(reconciliation) != {"receiptPath", "receiptSha256"}
        or not all(isinstance(v, str) and v for v in reconciliation.values())
    ):
        errors.append("Release unit baselineReconciliation is malformed")
    identifier = unit.get("id")
    if not isinstance(identifier, str) or not RELEASE_UNIT_ID.fullmatch(identifier):
        errors.append("Release unit id is invalid")
    if unit.get("status") not in {"draft", "review", "frozen"}:
        errors.append(f"Release unit {identifier}.status is invalid")
    for field in ("objective", "decisionRef"):
        if not isinstance(unit.get(field), str) or not unit[field].strip():
            errors.append(f"Release unit {identifier}.{field} is required")
    dependencies = unit.get("dependsOnReleaseUnits")
    if not isinstance(dependencies, list) or not all(
        isinstance(item, str) and item for item in dependencies
    ) or len(dependencies) != len(set(dependencies)):
        errors.append(
            f"Release unit {identifier}.dependsOnReleaseUnits must contain unique IDs"
        )
    superseded = unit.get("supersedesReleaseUnits", [])
    if not isinstance(superseded, list) or not all(
        isinstance(item, str) and item for item in superseded
    ) or len(superseded) != len(set(superseded)):
        errors.append(
            f"Release unit {identifier}.supersedesReleaseUnits must contain unique IDs"
        )
    elif isinstance(dependencies, list) and not set(superseded).issubset(
        set(dependencies)
    ):
        errors.append(
            f"Release unit {identifier}.supersedesReleaseUnits must be a subset of "
            "dependsOnReleaseUnits"
        )
    roots = unit.get("roots")
    if not isinstance(roots, dict):
        errors.append(f"Release unit {identifier}.roots must be an object")
    else:
        unexpected = set(roots) - set(ROOT_KEYS)
        if unexpected:
            errors.append(
                f"Release unit {identifier}.roots has unsupported kinds: {sorted(unexpected)}"
            )
        for kind, values in roots.items():
            if not isinstance(values, list) or not all(
                isinstance(item, str) and item for item in values
            ) or len(values) != len(set(values)):
                errors.append(
                    f"Release unit {identifier}.roots.{kind} must contain unique IDs"
                )
        if not isinstance(roots.get("workPackages"), list) or not roots["workPackages"]:
            errors.append(
                f"Release unit {identifier}.roots.workPackages must be non-empty"
            )
    status = unit.get("status")
    if status in {"draft", "review"}:
        for field in ("closure", "deferredSnapshot", "freeze"):
            if unit.get(field) is not None:
                errors.append(
                    f"Release unit {identifier}.{field} must be null before sealing"
                )
    if status == "frozen":
        closure = unit.get("closure")
        if not isinstance(closure, dict):
            errors.append(f"Frozen release unit {identifier} requires closure")
        else:
            if set(closure) != set(CLOSURE_KEYS):
                errors.append(
                    f"Frozen release unit {identifier}.closure kinds must be {sorted(CLOSURE_KEYS)}"
                )
            for kind, values in closure.items():
                if not isinstance(values, list) or not all(
                    isinstance(item, str) and item for item in values
                ) or values != sorted(set(values)):
                    errors.append(
                        f"Frozen release unit {identifier}.closure.{kind} must be a sorted unique ID array"
                    )
            if isinstance(roots, dict):
                for kind, values in roots.items():
                    if isinstance(values, list) and not set(values).issubset(
                        set(closure.get(kind, []))
                    ):
                        errors.append(
                            f"Frozen release unit {identifier}.closure omits roots.{kind}"
                        )
        deferred = unit.get("deferredSnapshot")
        if not isinstance(deferred, dict):
            errors.append(f"Frozen release unit {identifier} requires deferredSnapshot")
        else:
            if set(deferred) != {"schemaVersion", "catalogHash", "items"}:
                errors.append(
                    f"Frozen release unit {identifier}.deferredSnapshot fields are invalid"
                )
            if deferred.get("schemaVersion") != 1:
                errors.append(
                    f"Frozen release unit {identifier}.deferredSnapshot schemaVersion must be 1"
                )
            if not isinstance(deferred.get("catalogHash"), str) or not re.fullmatch(
                r"sha256:[0-9a-f]{64}", str(deferred.get("catalogHash", ""))
            ):
                errors.append(
                    f"Frozen release unit {identifier}.deferredSnapshot.catalogHash is invalid"
                )
            items = deferred.get("items")
            if not isinstance(items, list):
                errors.append(
                    f"Frozen release unit {identifier}.deferredSnapshot.items must be an array"
                )
            else:
                expected_item_fields = {"subjectKind", "subjectId", "message"}
                for index, item in enumerate(items):
                    if not isinstance(item, dict) or set(item) != expected_item_fields:
                        errors.append(
                            f"Frozen release unit {identifier}.deferredSnapshot.items[{index}] is invalid"
                        )
                    elif not all(
                        isinstance(item[field], str) and item[field]
                        for field in expected_item_fields
                    ):
                        errors.append(
                            f"Frozen release unit {identifier}.deferredSnapshot.items[{index}] fields are required"
                        )
        freeze = unit.get("freeze")
        if not isinstance(freeze, dict):
            errors.append(f"Frozen release unit {identifier} requires freeze")
        else:
            expected = {
                "releaseUnitId",
                "decisionId",
                "commit",
                "frozenAt",
                "releaseUnitHash",
                "snapshotPath",
            }
            if set(freeze) != expected:
                errors.append(
                    f"Frozen release unit {identifier}.freeze fields must be {sorted(expected)}"
                )
            for field in expected:
                if not isinstance(freeze.get(field), str) or not freeze[field]:
                    errors.append(
                        f"Frozen release unit {identifier}.freeze.{field} is required"
                    )
            if freeze.get("releaseUnitId") != identifier:
                errors.append(
                    f"Frozen release unit {identifier}.freeze.releaseUnitId mismatch"
                )
            if freeze.get("decisionId") != unit.get("decisionRef"):
                errors.append(
                    f"Frozen release unit {identifier}.freeze.decisionId mismatch"
                )
            digest = freeze.get("releaseUnitHash")
            if not isinstance(digest, str) or not re.fullmatch(
                r"sha256:[0-9a-f]{64}", digest
            ):
                errors.append(
                    f"Frozen release unit {identifier}.freeze.releaseUnitHash is invalid"
                )
            elif freeze.get("snapshotPath") != release_unit_snapshot_relative(digest):
                errors.append(
                    f"Frozen release unit {identifier}.freeze.snapshotPath is not derived"
                )
    return errors


def release_unit_index(data: dict[str, Any]) -> dict[str, dict[str, Any]]:
    if data.get("schemaVersion") != 1:
        raise ValueError("release-units.json schemaVersion must be 1")
    units = data.get("releaseUnits")
    if not isinstance(units, list):
        raise ValueError("release-units.json releaseUnits must be an array")
    result: dict[str, dict[str, Any]] = {}
    for index, unit in enumerate(units):
        issues = validate_release_unit_shape(unit)
        if issues:
            raise ValueError(f"releaseUnits[{index}]: " + "; ".join(issues))
        identifier = str(unit["id"])
        if identifier in result:
            raise ValueError(f"Duplicate release unit: {identifier}")
        result[identifier] = unit
    for identifier, unit in result.items():
        for dependency in unit.get("dependsOnReleaseUnits", []):
            if dependency not in result:
                raise ValueError(
                    f"Release unit {identifier} has missing dependency {dependency}"
                )
            if dependency == identifier:
                raise ValueError(f"Release unit {identifier} cannot depend on itself")
        for superseded in unit.get("supersedesReleaseUnits", []):
            target = result.get(superseded)
            if target is None:
                raise ValueError(
                    f"Release unit {identifier} supersedes missing release unit {superseded}"
                )
            if target.get("status") != "frozen":
                raise ValueError(
                    f"Release unit {identifier} can supersede only frozen release unit {superseded}"
                )
    direct_superseders: dict[str, list[str]] = {}
    for identifier, unit in result.items():
        for superseded in unit.get("supersedesReleaseUnits", []):
            direct_superseders.setdefault(superseded, []).append(identifier)
    for superseded, successors in direct_superseders.items():
        if len(successors) > 1:
            raise ValueError(
                f"Release unit {superseded} has multiple direct superseders: "
                f"{sorted(successors)}"
            )
    visiting: set[str] = set()
    visited: set[str] = set()

    def visit(identifier: str, trail: list[str]) -> None:
        if identifier in visiting:
            cycle = trail[trail.index(identifier) :] + [identifier]
            raise ValueError(f"Release-unit dependency cycle: {' -> '.join(cycle)}")
        if identifier in visited:
            return
        visiting.add(identifier)
        trail.append(identifier)
        for dependency in result[identifier].get("dependsOnReleaseUnits", []):
            visit(dependency, trail)
        trail.pop()
        visiting.remove(identifier)
        visited.add(identifier)

    for identifier in result:
        visit(identifier, [])
    return result


def _string_refs(value: Any, label: str) -> list[str]:
    if value is None:
        return []
    if not isinstance(value, list) or not all(isinstance(item, str) and item for item in value):
        raise ValueError(f"{label} must be an array of IDs")
    return list(value)


def compute_release_unit_closure(
    manifest: dict[str, Any],
    artifacts: dict[str, dict[str, Any]],
    unit: dict[str, Any],
) -> dict[str, list[str]]:
    """Close every supported canonical reference; reject unknown references."""

    indexes = artifact_indexes(manifest, artifacts)
    release_units_data = artifacts.get("releaseUnits")
    if not isinstance(release_units_data, dict):
        raise ValueError("Release units require artifact releaseUnits")
    release_units = release_unit_index(release_units_data)
    declared_release_dependencies = unit.get("dependsOnReleaseUnits", [])
    if not isinstance(declared_release_dependencies, list):
        raise ValueError("dependsOnReleaseUnits must be an array")
    externally_satisfied_packages: dict[str, str] = {}
    frozen_package_owners: dict[str, set[str]] = {}
    for release_id, release in release_units.items():
        if release.get("status") != "frozen":
            continue
        closure = release.get("closure")
        if not isinstance(closure, dict) or not isinstance(
            closure.get("workPackages"), list
        ):
            raise ValueError(
                f"Frozen release unit {release_id} lacks a work-package closure"
            )
        for package_id in closure["workPackages"]:
            if not isinstance(package_id, str) or not package_id:
                raise ValueError(
                    f"Frozen release unit {release_id} has an invalid work-package closure"
                )
            frozen_package_owners.setdefault(package_id, set()).add(release_id)
    for dependency_id in declared_release_dependencies:
        dependency = release_units.get(dependency_id)
        if dependency is None:
            raise ValueError(
                f"Release unit {unit.get('id')} has missing dependency {dependency_id}"
            )
        if dependency.get("status") != "frozen":
            raise ValueError(
                f"Release unit {unit.get('id')} dependency {dependency_id} is not frozen"
            )
        for package_id in dependency["closure"]["workPackages"]:
            previous = externally_satisfied_packages.get(package_id)
            if previous is not None and previous != dependency_id:
                raise ValueError(
                    f"External work package {package_id} is supplied by multiple dependency units"
                )
            externally_satisfied_packages[package_id] = dependency_id
    resources: dict[str, str] = {}
    for domain_id, domain in indexes["domains"].items():
        for resource in _string_refs(domain.get("owns"), f"Domain {domain_id}.owns"):
            if resource in resources and resources[resource] != domain_id:
                raise ValueError(f"Canonical resource {resource} has duplicate owners")
            resources[resource] = domain_id
    indexes["resources"] = {key: {"id": key, "owner": owner} for key, owner in resources.items()}

    selected: dict[str, set[str]] = {key: set() for key in CLOSURE_KEYS}
    queue: deque[tuple[str, str]] = deque()

    def add(kind: str, identifier: Any, reason: str) -> None:
        if identifier is None:
            return
        if not isinstance(identifier, str) or not identifier:
            raise ValueError(f"{reason} contains an invalid {kind} reference")
        if kind not in indexes:
            raise ValueError(f"Unsupported release-unit closure kind: {kind}")
        if identifier not in indexes[kind]:
            raise ValueError(f"{reason} references missing {kind} {identifier}")
        if identifier not in selected[kind]:
            selected[kind].add(identifier)
            queue.append((kind, identifier))

    opaque_reference_fields = {
        "minorPolicyRef",
        "vendorRefs",
        "forbiddenVendorRefs",
        "modelPolicyRef",
        "budgetPolicyRef",
        "evalDatasetRef",
        "processorRef",
        "processorRefs",
        "storageProcessorRefs",
        "executionProcessorRefs",
        "benchmarkProcessorRefs",
        "forbiddenProcessorRefs",
        "retentionPolicyRef",
        "commercialBoundaryRef",
        "qualityPolicyRef",
    }
    exact_reference_kinds = {
        "actorRef": "actors",
        "stateProfileRef": "pageStateProfiles",
        "surfaceRef": "pages",
        "basisRefs": "evidence",
        "sourceRefs": "evidence",
        "capabilityRef": "entitlements",
        "capabilityRefs": "entitlements",
    }

    def close_declared_reference_fields(
        value: Any, reason: str, path: tuple[str, ...] = ()
    ) -> None:
        if isinstance(value, list):
            for index, child in enumerate(value):
                close_declared_reference_fields(child, reason, (*path, str(index)))
            return
        if not isinstance(value, dict):
            return
        for field, child in value.items():
            field_path = (*path, str(field))
            field_lower = str(field).casefold()
            if field in {"inputSchema", "outputSchema"}:
                continue
            target_kind = exact_reference_kinds.get(field)
            if target_kind is None:
                if field_lower.endswith("decisionref") or field_lower.endswith("decisionrefs"):
                    target_kind = "decisions"
                elif field_lower.endswith("requirementref") or field_lower.endswith("requirementrefs"):
                    target_kind = "requirements"
                elif field_lower.endswith("contractref") or field_lower.endswith("contractrefs"):
                    target_kind = "contracts"
                elif field_lower.endswith("statemachineref") or field_lower.endswith("statemachinerefs"):
                    target_kind = "stateMachines"
                elif field_lower.endswith("evidenceref") or field_lower.endswith("evidencerefs"):
                    target_kind = "evidence"
                elif field_lower.endswith("pageref") or field_lower.endswith("pagerefs"):
                    target_kind = "pages"
                elif field_lower.endswith("domainref") or field_lower.endswith("domainrefs"):
                    target_kind = "domains"
            if target_kind is not None:
                refs = child if isinstance(child, list) else [child]
                for ref in refs:
                    if (
                        target_kind == "stateMachines"
                        and isinstance(ref, str)
                        and ref.startswith("not-applicable:")
                    ):
                        continue
                    add(
                        target_kind,
                        ref,
                        f"{reason}.{'.'.join(field_path)}",
                    )
            elif field_lower.endswith(("ref", "refs")) and field not in opaque_reference_fields:
                raise ValueError(
                    f"{reason}.{'.'.join(field_path)} uses unsupported reference field {field}"
                )
            close_declared_reference_fields(child, reason, field_path)

    roots = unit.get("roots", {})
    explicit_domain_roots = set(
        _string_refs(roots.get("domains", []), "Release unit roots.domains")
    )
    add("decisions", unit.get("decisionRef"), f"Release unit {unit.get('id')}")
    for kind in ROOT_KEYS:
        for identifier in _string_refs(roots.get(kind, []), f"Release unit roots.{kind}"):
            if kind == "workPackages" and identifier in externally_satisfied_packages:
                raise ValueError(
                    f"Release unit {unit.get('id')} roots work package {identifier} is already "
                    f"satisfied by dependency unit {externally_satisfied_packages[identifier]}"
                )
            add(kind, identifier, f"Release unit {unit.get('id')}")

    while queue:
        kind, identifier = queue.popleft()
        item = indexes[kind][identifier]
        close_declared_reference_fields(item, f"{kind} {identifier}")
        if kind == "workPackages":
            for ref in _string_refs(item.get("dependsOn", []), f"Work package {identifier}.dependsOn"):
                if ref in externally_satisfied_packages:
                    continue
                frozen_owners = frozen_package_owners.get(ref, set())
                if frozen_owners:
                    raise ValueError(
                        f"Work package {identifier} dependency {ref} is owned by frozen release "
                        f"unit(s) {sorted(frozen_owners)} but no supplying unit is declared in "
                        "dependsOnReleaseUnits"
                    )
                add("workPackages", ref, f"Work package {identifier}")
            for ref in _string_refs(item.get("requirementRefs", []), f"Work package {identifier}.requirementRefs"):
                add("requirements", ref, f"Work package {identifier}")
            for field in ("decisionRefs", "blockingDecisionRefs"):
                for ref in _string_refs(item.get(field, []), f"Work package {identifier}.{field}"):
                    add("decisions", ref, f"Work package {identifier}")
            for field in ("inputContracts", "outputContracts"):
                for ref in _string_refs(item.get(field, []), f"Work package {identifier}.{field}"):
                    add("contracts", ref, f"Work package {identifier}")
        elif kind in {"requirements", "nonFunctionalRequirements"}:
            for ref in _string_refs(item.get("evidenceRefs", []), f"{kind} {identifier}.evidenceRefs"):
                add("evidence", ref, f"{kind} {identifier}")
            if isinstance(item.get("owner"), str):
                add("domains", item["owner"], f"{kind} {identifier}")
            for field in ("decisionRef", "blockingDecisionRef"):
                if isinstance(item.get(field), str):
                    add("decisions", item[field], f"{kind} {identifier}")
        elif kind == "domains":
            if identifier in explicit_domain_roots:
                for ref in _string_refs(item.get("owns", []), f"Domain {identifier}.owns"):
                    add("resources", ref, f"Domain {identifier}")
                for field in ("commands", "queries", "emits", "consumes"):
                    for ref in _string_refs(item.get(field, []), f"Domain {identifier}.{field}"):
                        add("contracts", ref, f"Domain {identifier}")
            for track in _string_refs(item.get("trackScope", []), f"Domain {identifier}.trackScope"):
                if track != "shared":
                    add("tracks", track, f"Domain {identifier}")
            for term_id, term in indexes["glossaryTerms"].items():
                if term.get("owner") == identifier:
                    add("glossaryTerms", term_id, f"Domain {identifier}")
        elif kind == "contracts":
            add("domains", item.get("owner"), f"Contract {identifier}")
            for ref in _string_refs(item.get("requirementRefs", []), f"Contract {identifier}.requirementRefs"):
                add("requirements", ref, f"Contract {identifier}")
            for ref in _string_refs(item.get("evidenceRefs", []), f"Contract {identifier}.evidenceRefs"):
                add("evidence", ref, f"Contract {identifier}")
            for ref in _string_refs(item.get("transactionScopeResources", []), f"Contract {identifier}.transactionScopeResources"):
                add("resources", ref, f"Contract {identifier}")
            for ref in _string_refs(item.get("transactionEmits", []), f"Contract {identifier}.transactionEmits"):
                add("contracts", ref, f"Contract {identifier}")
            for machine_id, machine in indexes["stateMachines"].items():
                transitions = machine.get("transitions", [])
                if isinstance(transitions, list) and any(
                    isinstance(transition, dict)
                    and (
                        transition.get("command") == identifier
                        or identifier in transition.get("emits", [])
                    )
                    for transition in transitions
                ):
                    add("stateMachines", machine_id, f"Contract {identifier}")
        elif kind == "stateMachines":
            add("domains", item.get("owner"), f"State machine {identifier}")
            add("resources", item.get("resource"), f"State machine {identifier}")
            transitions = item.get("transitions")
            if not isinstance(transitions, list):
                raise ValueError(f"State machine {identifier}.transitions must be an array")
            for transition in transitions:
                if not isinstance(transition, dict):
                    raise ValueError(f"State machine {identifier} has an invalid transition")
                add("contracts", transition.get("command"), f"State machine {identifier}")
                for ref in _string_refs(transition.get("emits", []), f"State machine {identifier}.emits"):
                    add("contracts", ref, f"State machine {identifier}")
                for ref in _string_refs(transition.get("writesResources", []), f"State machine {identifier}.writesResources"):
                    add("resources", ref, f"State machine {identifier}")
        elif kind == "pages":
            for ref in _string_refs(item.get("dataOwners", []), f"Page {identifier}.dataOwners"):
                add("domains", ref, f"Page {identifier}")
            for track in _string_refs(item.get("trackScope", []), f"Page {identifier}.trackScope"):
                if track != "shared":
                    add("tracks", track, f"Page {identifier}")
            if isinstance(item.get("stateProfileRef"), str):
                add("pageStateProfiles", item["stateProfileRef"], f"Page {identifier}")
            for state in (item.get("states") or {}).values():
                if isinstance(state, dict) and isinstance(state.get("decisionRef"), str):
                    add("decisions", state["decisionRef"], f"Page {identifier}")
        elif kind == "pageStateProfiles":
            states = item.get("states")
            if isinstance(states, dict):
                for state in states.values():
                    if isinstance(state, dict) and isinstance(state.get("decisionRef"), str):
                        add("decisions", state["decisionRef"], f"State profile {identifier}")
        elif kind == "journeys":
            add("actors", item.get("actorRef"), f"Journey {identifier}")
            for ref in _string_refs(item.get("requirementRefs", []), f"Journey {identifier}.requirementRefs"):
                add("requirements", ref, f"Journey {identifier}")
            steps = item.get("steps")
            if not isinstance(steps, list):
                raise ValueError(f"Journey {identifier}.steps must be an array")
            for step in steps:
                if not isinstance(step, dict):
                    raise ValueError(f"Journey {identifier} has an invalid step")
                add("pages", step.get("surfaceRef"), f"Journey {identifier}")
        elif kind == "dataLifecycleResources":
            add("domains", item.get("owner"), f"Data lifecycle {identifier}")
            for ref in _string_refs(item.get("evidenceRefs", []), f"Data lifecycle {identifier}.evidenceRefs"):
                add("evidence", ref, f"Data lifecycle {identifier}")
            if isinstance(item.get("decisionRef"), str):
                add("decisions", item["decisionRef"], f"Data lifecycle {identifier}")
            for ref in _string_refs(item.get("derivedFrom", []), f"Data lifecycle {identifier}.derivedFrom"):
                add("resources", ref, f"Data lifecycle {identifier}")
        elif kind == "aiScenarios":
            for field in ("owner", "businessOwner", "factOwner"):
                add("domains", item.get(field), f"AI scenario {identifier}")
            for ref in _string_refs(item.get("inputContracts", []), f"AI scenario {identifier}.inputContracts"):
                add("contracts", ref, f"AI scenario {identifier}")
            add("contracts", item.get("outputContract"), f"AI scenario {identifier}")
            for field in (
                "blockingDecisionRefs",
                "factAuthorityDecisionRef",
                "confirmationDecisionRef",
            ):
                value = item.get(field)
                refs = value if isinstance(value, list) else [value] if isinstance(value, str) else []
                for ref in refs:
                    add("decisions", ref, f"AI scenario {identifier}")
            for ref in _string_refs(item.get("evidenceRefs", []), f"AI scenario {identifier}.evidenceRefs"):
                add("evidence", ref, f"AI scenario {identifier}")
            quality = item.get("qualityGate")
            if isinstance(quality, dict):
                for ref in _string_refs(quality.get("evidenceRefs", []), f"AI scenario {identifier}.qualityGate.evidenceRefs"):
                    add("evidence", ref, f"AI scenario {identifier}")
        elif kind == "entitlements":
            for field in ("owner", "businessOwner"):
                add("domains", item.get(field), f"Entitlement {identifier}")
            add("requirements", item.get("requirementRef"), f"Entitlement {identifier}")
            for field in ("decisionRef", "blockingDecisionRef"):
                if isinstance(item.get(field), str):
                    add("decisions", item[field], f"Entitlement {identifier}")
            for ref in _string_refs(item.get("contractRefs", []), f"Entitlement {identifier}.contractRefs"):
                add("contracts", ref, f"Entitlement {identifier}")
            state_machine_ref = item.get("reservationStateMachineRef")
            if isinstance(state_machine_ref, str) and state_machine_ref in indexes["stateMachines"]:
                add("stateMachines", state_machine_ref, f"Entitlement {identifier}")
        elif kind == "trackCoverage":
            coverage = item["item"]
            add("tracks", item["track"], f"Track coverage {identifier}")
            if isinstance(coverage.get("producer"), str):
                add("domains", coverage["producer"], f"Track coverage {identifier}")
            if isinstance(coverage.get("contract"), str):
                add("contracts", coverage["contract"], f"Track coverage {identifier}")
            for ref in _string_refs(coverage.get("evidenceRefs", []), f"Track coverage {identifier}.evidenceRefs"):
                add("evidence", ref, f"Track coverage {identifier}")
            if isinstance(coverage.get("decisionRef"), str):
                add("decisions", coverage["decisionRef"], f"Track coverage {identifier}")
            for consumer in _string_refs(coverage.get("consumers", []), f"Track coverage {identifier}.consumers"):
                if consumer in indexes["domains"]:
                    add("domains", consumer, f"Track coverage {identifier}")
                elif consumer in indexes["pages"]:
                    add("pages", consumer, f"Track coverage {identifier}")
        elif kind == "evidence":
            for ref in _string_refs(item.get("basisRefs", []), f"Evidence {identifier}.basisRefs"):
                add("evidence", ref, f"Evidence {identifier}")
        elif kind == "decisions":
            for ref in _string_refs(item.get("evidenceRefs", []), f"Decision {identifier}.evidenceRefs"):
                add("evidence", ref, f"Decision {identifier}")
        elif kind == "conflicts":
            for ref in _string_refs(item.get("sourceRefs", []), f"Conflict {identifier}.sourceRefs"):
                add("evidence", ref, f"Conflict {identifier}")
            if isinstance(item.get("decisionRef"), str):
                add("decisions", item["decisionRef"], f"Conflict {identifier}")
        elif kind == "glossaryTerms":
            add("domains", item.get("owner"), f"Glossary term {identifier}")
        elif kind == "resources":
            add("domains", item.get("owner"), f"Resource {identifier}")
            if identifier in indexes["dataLifecycleResources"]:
                add("dataLifecycleResources", identifier, f"Resource {identifier}")
            for machine_id, machine in indexes["stateMachines"].items():
                transitions = machine.get("transitions", [])
                if machine.get("resource") == identifier or (
                    isinstance(transitions, list)
                    and any(
                        isinstance(transition, dict)
                        and identifier in transition.get("writesResources", [])
                        for transition in transitions
                    )
                ):
                    add("stateMachines", machine_id, f"Resource {identifier}")
        elif kind == "tracks":
            decision_ref = manifest.get("trackDecisionRefs", {}).get(identifier)
            if isinstance(decision_ref, str):
                add("decisions", decision_ref, f"Track {identifier}")
        elif kind == "actors":
            pass
        else:
            raise ValueError(f"Closure behavior is not implemented for {kind}")

    return {key: sorted(selected[key]) for key in CLOSURE_KEYS}


def closure_contains(closure: dict[str, list[str]], kind: str, identifier: str) -> bool:
    return identifier in closure.get(kind, [])


def deferred_snapshot(
    bundle: Path,
    manifest: dict[str, Any],
    unresolved_items: Iterable[dict[str, str]],
) -> dict[str, Any]:
    items = sorted(
        (
            {
                "subjectKind": str(item.get("subjectKind", "catalog")),
                "subjectId": str(item.get("subjectId", "catalog")),
                "message": str(item["message"]),
            }
            for item in unresolved_items
        ),
        key=lambda item: (item["subjectKind"], item["subjectId"], item["message"]),
    )
    return {
        "schemaVersion": 1,
        "catalogHash": governance_hash(bundle, manifest),
        "items": items,
    }


def project_release_unit_payload(
    manifest: dict[str, Any],
    artifacts: dict[str, dict[str, Any]],
    unit: dict[str, Any],
    closure: dict[str, list[str]],
    deferred: dict[str, Any],
    *,
    commit: str,
    frozen_at: str,
) -> dict[str, Any]:
    indexes = artifact_indexes(manifest, artifacts)

    def selected(kind: str) -> list[dict[str, Any]]:
        return [indexes[kind][identifier] for identifier in closure[kind]]

    explicit_domain_roots = set(unit.get("roots", {}).get("domains", []))

    def selected_domains() -> list[dict[str, Any]]:
        result: list[dict[str, Any]] = []
        passive_fields = {"id", "owner", "scope", "trackScope", "invariants"}
        for identifier in closure["domains"]:
            domain = indexes["domains"][identifier]
            if identifier in explicit_domain_roots:
                result.append(domain)
            else:
                result.append(
                    {
                        key: value
                        for key, value in domain.items()
                        if key in passive_fields
                    }
                )
        return result

    coverage_by_capability: dict[str, dict[str, Any]] = {}
    for identifier in closure["trackCoverage"]:
        record = indexes["trackCoverage"][identifier]
        capability = record["capability"]
        target = coverage_by_capability.setdefault(
            capability, {"capability": capability, "tracks": {}}
        )
        target["tracks"][record["track"]] = record["item"]
    projected = {
        "evidenceLedger": {"schemaVersion": 1, "entries": selected("evidence")},
        "conflictRegister": {"schemaVersion": 1, "conflicts": selected("conflicts")},
        "decisionRegister": {"schemaVersion": 1, "decisions": selected("decisions")},
        "glossary": {"schemaVersion": 1, "terms": selected("glossaryTerms")},
        "requirements": {"schemaVersion": 1, "requirements": selected("requirements")},
        "journeys": {
            "schemaVersion": 1,
            "actors": selected("actors"),
            "journeys": selected("journeys"),
        },
        "domains": {"schemaVersion": 1, "domains": selected_domains()},
        "stateMachines": {"schemaVersion": 1, "machines": selected("stateMachines")},
        "contracts": {"schemaVersion": 1, "contracts": selected("contracts")},
        "pageMatrix": {
            "schemaVersion": 1,
            "stateProfiles": selected("pageStateProfiles"),
            "pages": selected("pages"),
        },
        "trackCoverage": {
            "schemaVersion": 1,
            "capabilities": [coverage_by_capability[key] for key in sorted(coverage_by_capability)],
        },
        "dataLifecycle": {
            "schemaVersion": 1,
            "resources": selected("dataLifecycleResources"),
        },
        "aiScenarios": {"schemaVersion": 1, "scenarios": selected("aiScenarios")},
        "entitlements": {"schemaVersion": 1, "capabilities": selected("entitlements")},
        "nonFunctionalRequirements": {
            "schemaVersion": 1,
            "requirements": selected("nonFunctionalRequirements"),
        },
        "workPackages": {"schemaVersion": 1, "packages": selected("workPackages")},
    }
    payload = {
        "schemaVersion": 1,
        "releaseUnitId": unit["id"],
        "releaseUnitHash": None,
        "decisionId": unit["decisionRef"],
        "objective": unit["objective"],
        "dependsOnReleaseUnits": list(unit.get("dependsOnReleaseUnits", [])),
        "supersedesReleaseUnits": list(unit.get("supersedesReleaseUnits", [])),
        "roots": unit["roots"],
        "commit": commit,
        "frozenAt": frozen_at,
        "catalogManifest": {
            key: manifest.get(key)
            for key in (
                "schemaVersion",
                "product",
                "authority",
                "tracks",
                "trackDecisionRefs",
                "requiredPageStates",
                "dualTrackCapabilities",
                "requiredNfrCategories",
            )
        },
        "closure": closure,
        "deferredSnapshot": deferred,
        "artifacts": projected,
    }
    if unit.get("baselineReconciliation") is not None:
        payload["baselineReconciliation"] = unit["baselineReconciliation"]
    payload["releaseUnitHash"] = release_unit_hash(payload)
    return payload


def load_release_unit_snapshot(
    root: Path, unit: dict[str, Any]
) -> dict[str, Any]:
    freeze = unit.get("freeze")
    if not isinstance(freeze, dict):
        raise ValueError(f"Release unit {unit.get('id')} is not frozen")
    expected_hash = freeze.get("releaseUnitHash")
    relative = freeze.get("snapshotPath")
    if not isinstance(expected_hash, str) or not isinstance(relative, str):
        raise ValueError(f"Release unit {unit.get('id')} freeze identity is incomplete")
    if relative != release_unit_snapshot_relative(expected_hash):
        raise ValueError(f"Release unit {unit.get('id')} snapshot path is not derived")
    path = bundle_execution_path(root, relative)
    if not path.is_file():
        raise ValueError(f"Release unit {unit.get('id')} snapshot is missing or unsafe")
    payload = read_object(path)
    if payload.get("releaseUnitId") != unit.get("id"):
        raise ValueError(f"Release unit {unit.get('id')} snapshot identity mismatch")
    if payload.get("releaseUnitHash") != expected_hash:
        raise ValueError(f"Release unit {unit.get('id')} snapshot hash field mismatch")
    if release_unit_hash(payload) != expected_hash:
        raise ValueError(f"Release unit {unit.get('id')} snapshot hash mismatch")
    for field, expected in {
        "decisionId": unit.get("decisionRef"), "objective": unit.get("objective"),
        "roots": unit.get("roots"),
        "dependsOnReleaseUnits": unit.get("dependsOnReleaseUnits", []),
        "supersedesReleaseUnits": unit.get("supersedesReleaseUnits", []),
        "commit": freeze.get("commit"), "frozenAt": freeze.get("frozenAt"),
    }.items():
        fallback = [] if field in {"dependsOnReleaseUnits", "supersedesReleaseUnits"} else None
        if payload.get(field, fallback) != expected:
            raise ValueError(f"Release unit {unit.get('id')} frozen authority {field} mismatch")
    if payload.get("closure") != unit.get("closure"):
        raise ValueError(f"Release unit {unit.get('id')} closure mismatch")
    if payload.get("deferredSnapshot") != unit.get("deferredSnapshot"):
        raise ValueError(f"Release unit {unit.get('id')} deferred snapshot mismatch")
    if payload.get("baselineReconciliation") != unit.get("baselineReconciliation"):
        raise ValueError(f"Release unit {unit.get('id')} baseline reconciliation mismatch")
    if payload.get("baselineReconciliation") is not None:
        from baseline_reconciliation import validate_archived_receipt
        validate_archived_receipt(root, payload)
    return payload


def validate_release_unit_snapshot_registry(
    root: Path, units: dict[str, dict[str, Any]]
) -> list[str]:
    """Require every immutable release-unit snapshot to remain indexed."""

    errors: list[str] = []
    snapshots_root = root / BUNDLE_DIR / "execution" / "release-unit-snapshots"
    if not snapshots_root.exists():
        return errors
    if snapshots_root.is_symlink() or not snapshots_root.is_dir():
        return ["Release-unit snapshot registry root is unsafe"]
    seen_units: dict[str, str] = {}
    for directory in sorted(snapshots_root.iterdir(), key=lambda item: item.name):
        if directory.is_symlink() or not directory.is_dir() or not re.fullmatch(
            r"[0-9a-f]{64}", directory.name
        ):
            errors.append(f"Unexpected release-unit snapshot registry entry: {directory.name}")
            continue
        relative = f"execution/release-unit-snapshots/{directory.name}/release-unit.json"
        try:
            path = bundle_execution_path(root, relative)
            if not path.is_file():
                raise ValueError("snapshot file is missing")
            payload = read_object(path)
            expected_hash = f"sha256:{directory.name}"
            if payload.get("releaseUnitHash") != expected_hash:
                raise ValueError("snapshot directory and hash field disagree")
            if release_unit_hash(payload) != expected_hash:
                raise ValueError("snapshot hash mismatch")
            unit_id = payload.get("releaseUnitId")
            if not isinstance(unit_id, str) or not unit_id:
                raise ValueError("snapshot releaseUnitId is missing")
            if unit_id in seen_units and seen_units[unit_id] != expected_hash:
                raise ValueError(
                    f"multiple immutable snapshots exist for release unit {unit_id}"
                )
            seen_units[unit_id] = expected_hash
            unit = units.get(unit_id)
            if unit is None or unit.get("status") != "frozen":
                raise ValueError(
                    f"immutable snapshot {expected_hash} is not indexed by a frozen release unit {unit_id}"
                )
            freeze = unit.get("freeze", {})
            if freeze.get("releaseUnitHash") != expected_hash:
                raise ValueError(
                    f"release unit {unit_id} no longer indexes immutable snapshot {expected_hash}"
                )
        except (OSError, ValueError, json.JSONDecodeError) as exc:
            errors.append(f"Release-unit snapshot {directory.name} is invalid: {exc}")
    for unit_id, unit in units.items():
        if unit.get("status") != "frozen":
            continue
        expected_hash = unit.get("freeze", {}).get("releaseUnitHash")
        if seen_units.get(unit_id) != expected_hash:
            errors.append(
                f"Frozen release unit {unit_id} lacks its indexed immutable snapshot"
            )
    return sorted(set(errors))


def snapshot_package(snapshot: dict[str, Any], package_id: str) -> dict[str, Any]:
    packages = snapshot.get("artifacts", {}).get("workPackages", {}).get("packages")
    if not isinstance(packages, list):
        raise ValueError("Release unit snapshot work packages are malformed")
    for package in packages:
        if isinstance(package, dict) and package.get("id") == package_id:
            return package
    raise ValueError(
        f"Work package {package_id} is outside release unit {snapshot.get('releaseUnitId')}"
    )


def paths_overlap(left: str, right: str) -> bool:
    def base(raw: str) -> tuple[str, ...]:
        normalized = raw.replace("\\", "/").strip()
        while normalized.startswith("./"):
            normalized = normalized[2:]
        for marker in ("/**", "/*"):
            if normalized.endswith(marker):
                normalized = normalized[: -len(marker)]
        return PurePosixPath(normalized or ".").parts

    left_parts = base(left)
    right_parts = base(right)
    return left_parts[: min(len(left_parts), len(right_parts))] == right_parts[
        : min(len(left_parts), len(right_parts))
    ]


def release_unit_supersedes(
    units: dict[str, dict[str, Any]], newer_id: str, older_id: str
) -> bool:
    """Return whether newer_id transitively replaces older_id's implementation authority."""

    if newer_id == older_id or newer_id not in units or older_id not in units:
        return False
    pending = list(units[newer_id].get("supersedesReleaseUnits", []))
    visited: set[str] = set()
    while pending:
        candidate = pending.pop()
        if candidate == older_id:
            return True
        if candidate in visited:
            continue
        visited.add(candidate)
        parent = units.get(candidate)
        if isinstance(parent, dict):
            pending.extend(parent.get("supersedesReleaseUnits", []))
    return False


def frozen_release_unit_superseders(
    units: dict[str, dict[str, Any]], release_unit_id: str
) -> list[str]:
    """List frozen units that transitively supersede one older authority."""

    return sorted(
        identifier
        for identifier, unit in units.items()
        if unit.get("status") == "frozen"
        and release_unit_supersedes(units, identifier, release_unit_id)
    )


def cross_unit_path_conflicts(
    root: Path,
    units: dict[str, dict[str, Any]],
    candidate_id: str,
    candidate_packages: Iterable[dict[str, Any]],
) -> list[str]:
    candidate_paths = [
        (str(package.get("id")), path)
        for package in candidate_packages
        for path in package.get("allowedPaths", [])
        if isinstance(package, dict) and isinstance(path, str)
    ]
    conflicts: list[str] = []
    for other_id, other in units.items():
        if other_id == candidate_id or other.get("status") != "frozen":
            continue
        if release_unit_supersedes(
            units, candidate_id, other_id
        ) or release_unit_supersedes(units, other_id, candidate_id):
            continue
        snapshot = load_release_unit_snapshot(root, other)
        packages = snapshot.get("artifacts", {}).get("workPackages", {}).get("packages", [])
        for package in packages:
            if not isinstance(package, dict):
                continue
            for path in package.get("allowedPaths", []):
                if not isinstance(path, str):
                    continue
                for candidate_package, candidate_path in candidate_paths:
                    if paths_overlap(path, candidate_path):
                        conflicts.append(
                            "Cross-release-unit allowedPaths overlap: "
                            f"{candidate_id}/{candidate_package}:{candidate_path} and "
                            f"{other_id}/{package.get('id')}:{path}"
                        )
    return sorted(set(conflicts))
