#!/usr/bin/env python3
"""Validate product architecture evidence, coverage, ownership, and work packages."""

from __future__ import annotations

import argparse
from datetime import datetime
from decimal import Decimal, InvalidOperation
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import sys
from typing import Any, Iterable

from execution_integrity import (
    REPORT_SCHEMA_VERSION,
    SUPPORTED_REPORT_SCHEMA_VERSIONS,
    run_execution_identity,
    validate_completed_run_integrity,
)
from governance_common import BUNDLE_DIR, governance_hash, read_object, write_object
from runner_adapters import validate_scenario_command
from release_units import (
    closure_contains,
    compute_release_unit_closure,
    cross_unit_path_conflicts,
    deferred_snapshot,
    frozen_release_unit_superseders,
    load_release_unit_snapshot,
    release_unit_index,
    snapshot_package,
    validate_release_unit_snapshot_registry,
)


SKILL_ROOT = Path(__file__).resolve().parent.parent
POLICY_PATH = SKILL_ROOT / "assets" / "default-policy.json"
VERSIONED_CONTRACT = re.compile(r"(?:@|[.:_-])v[1-9][0-9]*$", re.IGNORECASE)
WORK_PACKAGE_STATUSES = {"planned", "retired"}
WORK_PACKAGE_WAVES = {"contract", "producer", "consumer", "integration", "release"}


class Validation:
    def __init__(self, mode: str) -> None:
        self.mode = mode
        self.errors: list[str] = []
        self.warnings: list[str] = []
        self.unresolved_items: list[dict[str, str]] = []
        self.release_unit_context: dict[str, Any] | None = None

    def error(self, message: str) -> None:
        self.errors.append(message)

    def unresolved(
        self,
        message: str,
        subject_kind: str = "catalog",
        subject_id: str = "catalog",
    ) -> None:
        self.unresolved_items.append(
            {
                "message": message,
                "subjectKind": subject_kind,
                "subjectId": subject_id,
            }
        )
        if self.mode == "draft":
            self.warnings.append(message)
        else:
            self.errors.append(message)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project-root", required=True, type=Path)
    parser.add_argument(
        "--mode", choices=("draft", "review", "freeze"), default="review"
    )
    parser.add_argument(
        "--release-unit",
        help="Validate one implementation release unit without claiming full-product readiness.",
    )
    parser.add_argument("--json", action="store_true", dest="as_json")
    parser.add_argument(
        "--report",
        type=Path,
        help="Atomically write the full JSON validation payload to this path.",
    )
    return parser.parse_args()


def object_list(
    container: dict[str, Any], key: str, validation: Validation
) -> list[dict[str, Any]]:
    value = container.get(key)
    if not isinstance(value, list):
        validation.error(f"{key} must be an array")
        return []
    result: list[dict[str, Any]] = []
    for index, item in enumerate(value):
        if not isinstance(item, dict):
            validation.error(f"{key}[{index}] must be an object")
        else:
            result.append(item)
    return result


def unique_index(
    entries: Iterable[dict[str, Any]],
    key: str,
    label: str,
    validation: Validation,
) -> dict[str, dict[str, Any]]:
    result: dict[str, dict[str, Any]] = {}
    for index, entry in enumerate(entries):
        value = entry.get(key)
        if not isinstance(value, str) or not value.strip():
            validation.error(f"{label}[{index}].{key} must be a non-empty string")
            continue
        if value in result:
            validation.error(f"Duplicate {label} {key}: {value}")
        result[value] = entry
    return result


def non_empty_strings(value: Any) -> bool:
    return (
        isinstance(value, list)
        and bool(value)
        and all(isinstance(item, str) and item for item in value)
    )


def optional_string_list(value: Any) -> bool:
    return isinstance(value, list) and all(
        isinstance(item, str) and item for item in value
    )


def evidence_supports(
    refs: Any,
    evidence: dict[str, dict[str, Any]],
    *,
    allowed_classes: set[str],
    minimum_level: int,
) -> bool:
    if not non_empty_strings(refs):
        return False
    return any(
        evidence.get(ref, {}).get("status") == "active"
        and evidence.get(ref, {}).get("class") in allowed_classes
        and isinstance(evidence.get(ref, {}).get("verificationLevel"), int)
        and evidence[ref]["verificationLevel"] >= minimum_level
        for ref in refs
    )


def validate_evidence(
    data: dict[str, Any], policy: dict[str, Any], root: Path, validation: Validation
) -> dict[str, dict[str, Any]]:
    entries = object_list(data, "entries", validation)
    evidence = unique_index(entries, "id", "evidence", validation)
    allowed = set(policy["evidenceClasses"])
    for evidence_id, entry in evidence.items():
        evidence_class = entry.get("class")
        if evidence_class not in allowed:
            validation.error(f"Evidence {evidence_id} has an invalid class")
        for field in ("claim", "locator", "observedAt"):
            if not isinstance(entry.get(field), str) or not entry[field].strip():
                validation.error(f"Evidence {evidence_id}.{field} is required")
        observed_at = entry.get("observedAt")
        if isinstance(observed_at, str):
            try:
                datetime.fromisoformat(
                    observed_at[:-1] + "+00:00"
                    if observed_at.endswith("Z")
                    else observed_at
                )
            except ValueError:
                validation.error(f"Evidence {evidence_id}.observedAt must be ISO-8601")
        if entry.get("scope") not in {"current", "target", "proposal"}:
            validation.error(
                f"Evidence {evidence_id}.scope must be current, target, or proposal"
            )
        if entry.get("status") not in {"active", "disputed", "superseded"}:
            validation.error(
                f"Evidence {evidence_id}.status must be active, disputed, or superseded"
            )
        level = entry.get("verificationLevel")
        if not isinstance(level, int) or isinstance(level, bool) or not 0 <= level <= 6:
            validation.error(
                f"Evidence {evidence_id}.verificationLevel must be an integer from 0 to 6"
            )
        basis_refs = entry.get("basisRefs", [])
        if not optional_string_list(basis_refs):
            validation.error(
                f"Evidence {evidence_id}.basisRefs must be an array of evidence IDs"
            )
            basis_refs = []
        if evidence_class in {"inferred", "proposed"} and not basis_refs:
            validation.error(
                f"Evidence {evidence_id} {evidence_class} requires basisRefs"
            )
        if evidence_class == "external_verified" and not str(
            entry.get("locator", "")
        ).startswith(("https://", "http://")):
            validation.error(
                f"Evidence {evidence_id} external_verified requires a source URL"
            )
        if isinstance(level, int) and level >= 2:
            artifact = entry.get("artifact")
            if not isinstance(artifact, dict):
                validation.error(
                    f"Evidence {evidence_id} level {level} requires a hashed artifact"
                )
            else:
                relative = artifact.get("path")
                recorded_hash = artifact.get("sha256")
                if (
                    not isinstance(relative, str)
                    or not relative
                    or Path(relative).is_absolute()
                    or ".." in PurePosixPath(relative.replace("\\", "/")).parts
                    or any(token in relative for token in ("*", "?", "[", "]"))
                ):
                    validation.error(f"Evidence {evidence_id}.artifact.path is unsafe")
                else:
                    artifact_path = (root / relative).resolve()
                    if (
                        root not in (artifact_path, *artifact_path.parents)
                        or not artifact_path.is_file()
                    ):
                        validation.error(
                            f"Evidence {evidence_id} artifact does not exist inside the project"
                        )
                    elif not isinstance(
                        recorded_hash, str
                    ) or not recorded_hash.startswith("sha256:"):
                        validation.error(
                            f"Evidence {evidence_id}.artifact.sha256 is required"
                        )
                    else:
                        digest = hashlib.sha256(artifact_path.read_bytes()).hexdigest()
                        if recorded_hash != f"sha256:{digest}":
                            validation.error(
                                f"Evidence {evidence_id} artifact hash mismatch"
                            )
    for evidence_id, entry in evidence.items():
        for ref in entry.get("basisRefs", []):
            if ref not in evidence:
                validation.error(
                    f"Evidence {evidence_id} references missing basis evidence {ref}"
                )
            elif ref == evidence_id:
                validation.error(
                    f"Evidence {evidence_id} cannot cite itself as its basis"
                )
    return evidence


def validate_decisions(
    data: dict[str, Any], evidence: dict[str, dict[str, Any]], validation: Validation
) -> dict[str, dict[str, Any]]:
    decisions = unique_index(
        object_list(data, "decisions", validation), "id", "decision", validation
    )
    for decision_id, decision in decisions.items():
        status = decision.get("status")
        if status not in {"accepted", "rejected", "proposed", "superseded"}:
            validation.error(f"Decision {decision_id} has an invalid status")
        if (
            not isinstance(decision.get("question"), str)
            or not decision["question"].strip()
        ):
            validation.error(f"Decision {decision_id}.question is required")
        if status in {"accepted", "rejected", "superseded"}:
            for field in ("choice", "decidedBy", "decidedAt"):
                if (
                    not isinstance(decision.get(field), str)
                    or not decision[field].strip()
                ):
                    validation.error(
                        f"Decision {decision_id}.{field} is required for {status}"
                    )
        refs = decision.get("evidenceRefs", [])
        if not optional_string_list(refs):
            validation.error(
                f"Decision {decision_id}.evidenceRefs must be an array of IDs"
            )
        else:
            for ref in refs:
                if ref not in evidence:
                    validation.error(
                        f"Decision {decision_id} references missing evidence {ref}"
                    )
        if status in {"accepted", "rejected"} and not refs:
            validation.error(f"Decision {decision_id} {status} requires evidenceRefs")
    return decisions


def accepted_decision(reference: Any, decisions: dict[str, dict[str, Any]]) -> bool:
    return (
        isinstance(reference, str)
        and decisions.get(reference, {}).get("status") == "accepted"
    )


def validate_conflicts(
    data: dict[str, Any],
    evidence: dict[str, dict[str, Any]],
    decisions: dict[str, dict[str, Any]],
    validation: Validation,
) -> None:
    conflicts = unique_index(
        object_list(data, "conflicts", validation), "id", "conflict", validation
    )
    for conflict_id, conflict in conflicts.items():
        status = conflict.get("status")
        if status not in {"open", "resolved"}:
            validation.error(f"Conflict {conflict_id} has an invalid status")
        refs = conflict.get("sourceRefs")
        if not non_empty_strings(refs) or len(set(refs)) < 2:
            validation.error(
                f"Conflict {conflict_id}.sourceRefs must contain at least two evidence IDs"
            )
        else:
            for ref in refs:
                if ref not in evidence:
                    validation.error(
                        f"Conflict {conflict_id} references missing evidence {ref}"
                    )
        if status == "resolved" and not accepted_decision(
            conflict.get("decisionRef"), decisions
        ):
            validation.error(
                f"Resolved conflict {conflict_id} requires an accepted decision"
            )
        if status == "open" and conflict.get("blocking", True):
            validation.unresolved(
                f"Blocking conflict remains open: {conflict_id}",
                "conflicts",
                conflict_id,
            )


def validate_requirement_entries(
    entries: list[dict[str, Any]],
    label: str,
    policy: dict[str, Any],
    evidence: dict[str, dict[str, Any]],
    decisions: dict[str, dict[str, Any]],
    validation: Validation,
) -> dict[str, dict[str, Any]]:
    requirements = unique_index(entries, "id", label, validation)
    allowed_states = set(policy["requirementStates"])
    allowed_impacts = set(policy["impactLevels"])
    for requirement_id, requirement in requirements.items():
        state = requirement.get("state")
        impact = requirement.get("impact")
        if state not in allowed_states:
            validation.error(f"{label} {requirement_id} has an invalid state")
        if impact not in allowed_impacts:
            validation.error(f"{label} {requirement_id} has an invalid impact")
        if (
            not isinstance(requirement.get("statement"), str)
            or not requirement["statement"].strip()
        ):
            validation.error(f"{label} {requirement_id}.statement is required")
        if requirement.get("scope") not in {"current", "target", "proposal"}:
            validation.error(
                f"{label} {requirement_id}.scope must be current, target, or proposal"
            )
        refs = requirement.get("evidenceRefs", [])
        if not optional_string_list(refs):
            validation.error(f"{label} {requirement_id}.evidenceRefs must be an array")
        else:
            for ref in refs:
                if ref not in evidence:
                    validation.error(
                        f"{label} {requirement_id} references missing evidence {ref}"
                    )
        if state == "confirmed":
            if (
                not isinstance(requirement.get("owner"), str)
                or not requirement["owner"].strip()
            ):
                validation.error(
                    f"Confirmed {label} {requirement_id} requires an owner"
                )
            if not non_empty_strings(requirement.get("acceptance")):
                validation.error(
                    f"Confirmed {label} {requirement_id} requires acceptance evidence"
                )
            if not refs:
                validation.error(
                    f"Confirmed {label} {requirement_id} requires evidenceRefs"
                )
            if requirement.get("scope") == "current":
                if not evidence_supports(
                    refs, evidence, allowed_classes={"code_observed"}, minimum_level=1
                ):
                    validation.error(
                        f"Confirmed current {label} {requirement_id} requires observed code evidence"
                    )
            elif requirement.get("scope") == "target":
                if not evidence_supports(
                    refs,
                    evidence,
                    allowed_classes={"user_confirmed", "external_verified"},
                    minimum_level=0,
                ):
                    validation.error(
                        f"Confirmed target {label} {requirement_id} requires user-confirmed or external evidence"
                    )
            else:
                validation.error(
                    f"Confirmed {label} {requirement_id} cannot have proposal scope"
                )
        elif state == "not_applicable":
            if not accepted_decision(requirement.get("decisionRef"), decisions):
                validation.error(
                    f"Not-applicable {label} {requirement_id} requires an accepted decision"
                )
        elif impact in {"high", "critical"} and state in {
            "unknown",
            "conflict",
            "proposed",
        }:
            subject_kind = (
                "nonFunctionalRequirements"
                if label == "non-functional requirement"
                else "requirements"
            )
            validation.unresolved(
                f"High-impact {label} is unresolved: {requirement_id} ({state})",
                subject_kind,
                requirement_id,
            )
    return requirements


def validate_domains(
    data: dict[str, Any], tracks: set[str], validation: Validation
) -> dict[str, dict[str, Any]]:
    domains = unique_index(
        object_list(data, "domains", validation), "id", "domain", validation
    )
    owners: dict[str, str] = {}
    for domain_id, domain in domains.items():
        if not isinstance(domain.get("owner"), str) or not domain["owner"].strip():
            validation.error(f"Domain {domain_id}.owner is required")
        owned = domain.get("owns")
        if not non_empty_strings(owned):
            validation.error(
                f"Domain {domain_id}.owns must contain canonical resources"
            )
        else:
            for resource in owned:
                if resource in owners:
                    validation.error(
                        f"Canonical resource {resource} has duplicate owners: {owners[resource]}, {domain_id}"
                    )
                owners[resource] = domain_id
        scope = domain.get("trackScope", [])
        if not non_empty_strings(scope) or not set(scope).issubset(tracks | {"shared"}):
            validation.error(
                f"Domain {domain_id}.trackScope contains undeclared tracks"
            )
        if validation.mode != "draft" and not non_empty_strings(
            domain.get("invariants")
        ):
            validation.error(f"Domain {domain_id} requires explicit invariants")
        if validation.mode != "draft":
            for field in ("commands", "queries", "emits", "consumes"):
                if not isinstance(domain.get(field), list):
                    validation.error(f"Domain {domain_id}.{field} must be an array")
        emitted = (
            set(domain.get("emits", []))
            if isinstance(domain.get("emits"), list)
            else set()
        )
        consumed = (
            set(domain.get("consumes", []))
            if isinstance(domain.get("consumes"), list)
            else set()
        )
        self_consumed = emitted & consumed
        rationales = domain.get("selfConsumptionRationales", {})
        for event_id in sorted(self_consumed):
            rationale = (
                rationales.get(event_id) if isinstance(rationales, dict) else None
            )
            if not isinstance(rationale, str) or not rationale.strip():
                validation.error(
                    f"Domain {domain_id} self-consumes {event_id} without an explicit selfConsumptionRationale"
                )
    if not domains:
        validation.unresolved("No domains are defined", "domains", "*")
    return domains


def validate_requirement_owners(
    entries: dict[str, dict[str, Any]],
    label: str,
    domains: dict[str, dict[str, Any]],
    validation: Validation,
) -> None:
    for entry_id, entry in entries.items():
        if entry.get("state") == "confirmed" and entry.get("owner") not in domains:
            validation.error(
                f"Confirmed {label} {entry_id} references missing owner domain"
            )


def validate_nfr_verification(
    entries: dict[str, dict[str, Any]],
    policy: dict[str, Any],
    evidence: dict[str, dict[str, Any]],
    decisions: dict[str, dict[str, Any]],
    validation: Validation,
) -> None:
    minimum_levels = policy.get("nfrMinimumEvidenceLevels", {})
    for entry_id, entry in entries.items():
        if not non_empty_strings(entry.get("targetDimensions")):
            validation.error(f"NFR {entry_id}.targetDimensions must be non-empty")
        blocking_ref = entry.get("blockingDecisionRef")
        if not isinstance(blocking_ref, str) or blocking_ref not in decisions:
            validation.error(f"NFR {entry_id}.blockingDecisionRef is required")
        proposed = entry.get("proposedVerification")
        if not isinstance(proposed, dict):
            validation.error(f"NFR {entry_id}.proposedVerification must be an object")
        else:
            for field in (
                "condition",
                "method",
                "environment",
                "dataVolume",
                "evidenceArtifact",
                "reviewCadence",
            ):
                if (
                    not isinstance(proposed.get(field), str)
                    or not proposed[field].strip()
                ):
                    validation.error(
                        f"NFR {entry_id}.proposedVerification.{field} is required"
                    )
            artifact = proposed.get("evidenceArtifact")
            if (
                isinstance(artifact, str)
                and artifact
                and not safe_relative_file(artifact)
            ):
                validation.error(
                    f"NFR {entry_id}.proposedVerification.evidenceArtifact is unsafe"
                )
            category = entry.get("category")
            policy_level = minimum_levels.get(category)
            proposed_level = proposed.get("minimumEvidenceLevel")
            if (
                not isinstance(proposed_level, int)
                or isinstance(proposed_level, bool)
                or not isinstance(policy_level, int)
                or proposed_level < policy_level
                or proposed_level > 6
            ):
                validation.error(
                    f"NFR {entry_id}.proposedVerification.minimumEvidenceLevel must be "
                    f"between policy level {policy_level} and 6"
                )
        if entry.get("state") != "confirmed":
            continue
        verification = entry.get("verification")
        if not isinstance(verification, dict):
            validation.error(f"Confirmed NFR {entry_id}.verification must be an object")
            continue
        status = verification.get("status")
        if status not in {"planned", "implemented", "verified"}:
            validation.error(f"NFR {entry_id}.verification.status is invalid")
        for field in (
            "condition",
            "method",
            "environment",
            "dataVolume",
            "evidenceArtifact",
            "reviewCadence",
        ):
            value = verification.get(field)
            if not isinstance(value, str) or not value.strip():
                validation.error(f"NFR {entry_id}.verification.{field} is required")
        artifact = verification.get("evidenceArtifact")
        if isinstance(artifact, str) and artifact and not safe_relative_file(artifact):
            validation.error(f"NFR {entry_id}.verification.evidenceArtifact is unsafe")
        category = entry.get("category")
        policy_level = minimum_levels.get(category)
        declared_level = verification.get("minimumEvidenceLevel")
        if (
            not isinstance(declared_level, int)
            or isinstance(declared_level, bool)
            or not isinstance(policy_level, int)
            or declared_level < policy_level
            or declared_level > 6
        ):
            validation.error(
                f"NFR {entry_id}.verification.minimumEvidenceLevel must be "
                f"between policy level {policy_level} and 6"
            )
        refs = entry.get("evidenceRefs", [])
        if status == "implemented" and not evidence_supports(
            refs, evidence, allowed_classes={"code_observed"}, minimum_level=2
        ):
            validation.error(f"NFR {entry_id} implemented lacks executable evidence")
        if (
            status == "verified"
            and isinstance(declared_level, int)
            and not evidence_supports(
                refs,
                evidence,
                allowed_classes={"code_observed"},
                minimum_level=declared_level,
            )
        ):
            validation.error(
                f"NFR {entry_id} verified lacks evidence at level {declared_level}"
            )


def validate_glossary(
    data: dict[str, Any], domains: dict[str, dict[str, Any]], validation: Validation
) -> dict[str, dict[str, Any]]:
    terms = unique_index(
        object_list(data, "terms", validation), "id", "term", validation
    )
    claimed_names: dict[str, str] = {}
    for term_id, term in terms.items():
        canonical = term.get("term")
        if not isinstance(canonical, str) or not canonical.strip():
            validation.error(f"Term {term_id}.term is required")
            continue
        if (
            not isinstance(term.get("definition"), str)
            or not term["definition"].strip()
        ):
            validation.error(f"Term {term_id}.definition is required")
        if term.get("owner") not in domains:
            validation.error(f"Term {term_id} references missing owner domain")
        aliases = term.get("aliases", [])
        if not optional_string_list(aliases):
            validation.error(f"Term {term_id}.aliases must be an array")
            aliases = []
        for value in [canonical, *aliases]:
            normalized = value.casefold().strip()
            previous = claimed_names.get(normalized)
            if previous and previous != term_id:
                validation.error(
                    f"Glossary label {value!r} is claimed by both {previous} and {term_id}"
                )
            claimed_names[normalized] = term_id
    if not terms:
        validation.unresolved(
            "No canonical glossary terms are defined", "glossaryTerms", "*"
        )
    return terms


def domain_resource_owners(domains: dict[str, dict[str, Any]]) -> dict[str, str]:
    result: dict[str, str] = {}
    for domain_id, domain in domains.items():
        owned = domain.get("owns", [])
        if isinstance(owned, list):
            for resource in owned:
                if isinstance(resource, str):
                    result[resource] = domain_id
    return result


def validate_json_schema(value: Any, label: str, validation: Validation) -> None:
    """Validate the deliberately small JSON-Schema subset used by frozen contracts."""
    if not isinstance(value, dict):
        validation.error(f"{label} must be a JSON Schema object")
        return
    description = value.get("description")
    if not isinstance(description, str) or not description.strip():
        validation.error(f"{label}.description is required")
    schema_type = value.get("type")
    allowed_types = {
        "object",
        "array",
        "string",
        "integer",
        "number",
        "boolean",
        "null",
    }
    if schema_type not in allowed_types:
        validation.error(f"{label}.type must be one of {sorted(allowed_types)}")
        return
    allowed_keywords = {
        "type",
        "description",
        "enum",
        "const",
    }
    if schema_type == "object":
        allowed_keywords |= {"properties", "required", "additionalProperties", "oneOf"}
    elif schema_type == "array":
        allowed_keywords |= {"items", "minItems", "maxItems", "uniqueItems"}
    elif schema_type in {"integer", "number"}:
        allowed_keywords |= {"minimum", "maximum"}
    elif schema_type == "string":
        allowed_keywords |= {"minLength", "maxLength", "pattern", "format"}
    unknown_keywords = set(value) - allowed_keywords
    if unknown_keywords:
        validation.error(
            f"{label} uses unsupported JSON Schema keywords: {', '.join(sorted(unknown_keywords))}"
        )
    if schema_type == "object":
        properties = value.get("properties")
        required = value.get("required")
        if not isinstance(properties, dict):
            validation.error(f"{label}.properties must be an object")
            properties = {}
        if not optional_string_list(required):
            validation.error(f"{label}.required must be an array of property names")
            required = []
        missing = set(required) - set(properties)
        if missing:
            validation.error(
                f"{label}.required references missing properties: {', '.join(sorted(missing))}"
            )
        if value.get("additionalProperties") is not False:
            validation.error(f"{label}.additionalProperties must be false")
        for property_name, property_schema in properties.items():
            if not isinstance(property_name, str) or not property_name:
                validation.error(
                    f"{label}.properties contains an invalid property name"
                )
                continue
            validate_json_schema(
                property_schema, f"{label}.properties.{property_name}", validation
            )
        one_of = value.get("oneOf")
        if one_of is not None:
            if not isinstance(one_of, list) or len(one_of) < 2:
                validation.error(
                    f"{label}.oneOf must contain at least two schema branches"
                )
            else:
                for branch_index, branch in enumerate(one_of):
                    validate_json_schema(
                        branch, f"{label}.oneOf[{branch_index}]", validation
                    )
                    if isinstance(branch, dict) and branch.get("type") != schema_type:
                        validation.error(
                            f"{label}.oneOf[{branch_index}].type must equal parent type {schema_type}"
                        )
                    if isinstance(branch, dict) and branch.get("type") == "object":
                        branch_properties = branch.get("properties")
                        if isinstance(branch_properties, dict):
                            undeclared = set(branch_properties) - set(properties)
                            if undeclared:
                                validation.error(
                                    f"{label}.oneOf[{branch_index}] uses properties absent from the parent schema: {', '.join(sorted(undeclared))}"
                                )
                            forbidden_parent_required = set(required) - set(
                                branch_properties
                            )
                            if (
                                branch.get("additionalProperties") is False
                                and forbidden_parent_required
                            ):
                                validation.error(
                                    f"{label}.oneOf[{branch_index}] forbids parent-required properties: {', '.join(sorted(forbidden_parent_required))}"
                                )
                            for (
                                property_name,
                                branch_property,
                            ) in branch_properties.items():
                                parent_property = properties.get(property_name)
                                if not json_schema_parent_property_covers_branch(
                                    parent_property, branch_property
                                ):
                                    validation.error(
                                        f"{label}.properties.{property_name} narrows or contradicts oneOf[{branch_index}]"
                                    )
                for left_index in range(len(one_of)):
                    for right_index in range(left_index + 1, len(one_of)):
                        if not json_schema_branches_provably_disjoint(
                            one_of[left_index], one_of[right_index]
                        ):
                            validation.error(
                                f"{label}.oneOf branches {left_index} and {right_index} are not provably disjoint"
                            )
    elif schema_type == "array":
        validate_json_schema(value.get("items"), f"{label}.items", validation)
    enum = value.get("enum")
    if enum is not None and (not isinstance(enum, list) or not enum):
        validation.error(f"{label}.enum must be a non-empty array when present")
    elif isinstance(enum, list):
        normalized = [json_schema_value_key(item) for item in enum]
        if any(item is None for item in normalized):
            validation.error(f"{label}.enum contains a non-JSON or non-finite value")
        elif len(set(normalized)) != len(normalized):
            validation.error(f"{label}.enum contains duplicate JSON values")
        for item in enum:
            if not json_value_matches_schema_type(item, schema_type):
                validation.error(
                    f"{label}.enum contains a value incompatible with type {schema_type}"
                )
                break
    if "const" in value and not json_value_matches_schema_type(
        value["const"], schema_type
    ):
        validation.error(f"{label}.const is incompatible with type {schema_type}")
    if schema_type in {"integer", "number"}:
        minimum = value.get("minimum")
        maximum = value.get("maximum")
        for bound_name, bound in (("minimum", minimum), ("maximum", maximum)):
            if bound is not None and (
                not isinstance(bound, (int, float))
                or isinstance(bound, bool)
                or not json_number_is_finite(bound)
            ):
                validation.error(f"{label}.{bound_name} must be a finite number")
        if (
            isinstance(minimum, (int, float))
            and not isinstance(minimum, bool)
            and isinstance(maximum, (int, float))
            and not isinstance(maximum, bool)
            and minimum > maximum
        ):
            validation.error(f"{label}.minimum cannot exceed maximum")
    if schema_type == "array":
        minimum = value.get("minItems")
        maximum = value.get("maxItems")
        for bound_name, bound in (("minItems", minimum), ("maxItems", maximum)):
            if bound is not None and (
                not isinstance(bound, int) or isinstance(bound, bool) or bound < 0
            ):
                validation.error(f"{label}.{bound_name} must be a non-negative integer")
        if isinstance(minimum, int) and isinstance(maximum, int) and minimum > maximum:
            validation.error(f"{label}.minItems cannot exceed maxItems")
        if "uniqueItems" in value and not isinstance(value["uniqueItems"], bool):
            validation.error(f"{label}.uniqueItems must be boolean")
    if schema_type == "string":
        for bound_name in ("minLength", "maxLength"):
            bound = value.get(bound_name)
            if bound is not None and (
                not isinstance(bound, int) or isinstance(bound, bool) or bound < 0
            ):
                validation.error(f"{label}.{bound_name} must be a non-negative integer")
        pattern = value.get("pattern")
        if pattern is not None:
            if not isinstance(pattern, str):
                validation.error(f"{label}.pattern must be a string")
            else:
                try:
                    re.compile(pattern)
                except re.error:
                    validation.error(
                        f"{label}.pattern is not a valid regular expression"
                    )


def json_number_is_finite(value: int | float) -> bool:
    try:
        return Decimal(str(value)).is_finite()
    except InvalidOperation:
        return False


def json_value_matches_schema_type(value: Any, schema_type: str) -> bool:
    if schema_type == "object":
        return isinstance(value, dict)
    if schema_type == "array":
        return isinstance(value, list)
    if schema_type == "string":
        return isinstance(value, str)
    if schema_type == "boolean":
        return isinstance(value, bool)
    if schema_type == "null":
        return value is None
    if schema_type == "number":
        return (
            isinstance(value, (int, float))
            and not isinstance(value, bool)
            and json_number_is_finite(value)
        )
    if schema_type == "integer":
        if (
            not isinstance(value, (int, float))
            or isinstance(value, bool)
            or not json_number_is_finite(value)
        ):
            return False
        return Decimal(str(value)) == Decimal(str(value)).to_integral_value()
    return False


def json_schema_value_key(value: Any) -> tuple[Any, ...] | None:
    if value is None:
        return ("null",)
    if isinstance(value, bool):
        return ("boolean", value)
    if isinstance(value, (int, float)):
        if not json_number_is_finite(value):
            return None
        return ("number", Decimal(str(value)).normalize())
    if isinstance(value, str):
        return ("string", value)
    if isinstance(value, list):
        items = [json_schema_value_key(item) for item in value]
        if any(item is None for item in items):
            return None
        return ("array", *items)
    if isinstance(value, dict):
        items = []
        for key in sorted(value):
            if not isinstance(key, str):
                return None
            item_key = json_schema_value_key(value[key])
            if item_key is None:
                return None
            items.append((key, item_key))
        return ("object", *items)
    return None


def json_schema_fixed_values(schema: Any) -> set[tuple[Any, ...]] | None:
    if not isinstance(schema, dict):
        return None
    if "const" in schema:
        value = json_schema_value_key(schema["const"])
        return {value} if value is not None else None
    enum = schema.get("enum")
    if isinstance(enum, list) and enum:
        values = [json_schema_value_key(item) for item in enum]
        return set(values) if all(item is not None for item in values) else None
    return None


def json_schema_parent_property_covers_branch(parent: Any, branch: Any) -> bool:
    """Prove the parent declaration cannot invalidate a branch property.

    A parent property schema and a selected ``oneOf`` branch both apply.  This
    deliberately conservative proof prevents a copied discriminator, const or
    numeric bound at the parent level from silently making a branch impossible.
    """
    if not isinstance(parent, dict) or not isinstance(branch, dict):
        return False
    if parent.get("type") != branch.get("type"):
        return False

    parent_values = json_schema_fixed_values(parent)
    branch_values = json_schema_fixed_values(branch)
    if parent_values is not None:
        if branch_values is None or not branch_values.issubset(parent_values):
            return False

    for lower_key in ("minimum", "minItems"):
        if lower_key in parent:
            parent_lower = parent.get(lower_key)
            branch_lower = branch.get(lower_key)
            if not isinstance(parent_lower, (int, float)) or isinstance(
                parent_lower, bool
            ):
                return False
            if not isinstance(branch_lower, (int, float)) or isinstance(
                branch_lower, bool
            ):
                return False
            if parent_lower > branch_lower:
                return False
    for upper_key in ("maximum", "maxItems"):
        if upper_key in parent:
            parent_upper = parent.get(upper_key)
            branch_upper = branch.get(upper_key)
            if not isinstance(parent_upper, (int, float)) or isinstance(
                parent_upper, bool
            ):
                return False
            if not isinstance(branch_upper, (int, float)) or isinstance(
                branch_upper, bool
            ):
                return False
            if parent_upper < branch_upper:
                return False

    if parent.get("type") == "array":
        return json_schema_parent_property_covers_branch(
            parent.get("items"), branch.get("items")
        )
    if parent.get("type") == "object":
        parent_properties = parent.get("properties")
        branch_properties = branch.get("properties")
        if not isinstance(parent_properties, dict) or not isinstance(
            branch_properties, dict
        ):
            return False
        if not set(branch_properties).issubset(parent_properties):
            return False
        return all(
            json_schema_parent_property_covers_branch(
                parent_properties[name], branch_property
            )
            for name, branch_property in branch_properties.items()
        )
    return True


def json_schema_branches_provably_disjoint(left: Any, right: Any) -> bool:
    if not isinstance(left, dict) or not isinstance(right, dict):
        return False
    if left.get("type") != "object" or right.get("type") != "object":
        left_values = json_schema_fixed_values(left)
        right_values = json_schema_fixed_values(right)
        return (
            left_values is not None
            and right_values is not None
            and left_values.isdisjoint(right_values)
        )
    left_properties = left.get("properties")
    right_properties = right.get("properties")
    left_required = left.get("required")
    right_required = right.get("required")
    if not isinstance(left_properties, dict) or not isinstance(right_properties, dict):
        return False
    if not isinstance(left_required, list) or not isinstance(right_required, list):
        return False
    if left.get("additionalProperties") is False and any(
        isinstance(name, str) and name not in left_properties for name in right_required
    ):
        return True
    if right.get("additionalProperties") is False and any(
        isinstance(name, str) and name not in right_properties for name in left_required
    ):
        return True
    for name in set(left_required) & set(right_required):
        left_values = json_schema_fixed_values(left_properties.get(name))
        right_values = json_schema_fixed_values(right_properties.get(name))
        if (
            left_values is not None
            and right_values is not None
            and left_values.isdisjoint(right_values)
        ):
            return True
    return False


PLACEHOLDER_SCHEMA_PATTERNS = (
    "draft command envelope",
    "draft business payload",
    "draft projection envelope",
    "draft immutable payload references",
    "exact domain fields must be completed",
    "reference-only draft",
    "placeholder",
)


def schema_contains_placeholder(value: Any) -> bool:
    if isinstance(value, dict):
        description = value.get("description")
        if isinstance(description, str):
            normalized = description.casefold()
            if any(pattern in normalized for pattern in PLACEHOLDER_SCHEMA_PATTERNS):
                return True
        return any(schema_contains_placeholder(item) for item in value.values())
    if isinstance(value, list):
        return any(schema_contains_placeholder(item) for item in value)
    return False


def validate_domain_contract_references(
    domains: dict[str, dict[str, Any]],
    contracts: dict[str, dict[str, Any]],
    validation: Validation,
) -> None:
    expected_kinds = {
        "commands": "command",
        "queries": "query",
        "emits": "event",
        "consumes": "event",
    }
    for domain_id, domain in domains.items():
        for field, expected_kind in expected_kinds.items():
            references = domain.get(field, [])
            if not isinstance(references, list):
                continue
            for contract_id in references:
                contract = contracts.get(contract_id)
                if contract is None:
                    validation.unresolved(
                        f"Domain {domain_id}.{field} references missing contract {contract_id}",
                        "domains",
                        domain_id,
                    )
                    continue
                if contract.get("kind") != expected_kind:
                    validation.error(
                        f"Domain {domain_id}.{field} contract {contract_id} must be {expected_kind}"
                    )
                if field != "consumes" and contract.get("owner") != domain_id:
                    validation.error(
                        f"Domain {domain_id}.{field} does not own contract {contract_id}"
                    )
                if field == "consumes" and domain_id not in contract.get(
                    "consumers", []
                ):
                    validation.error(
                        f"Consumed event {contract_id} does not declare {domain_id} as a consumer"
                    )


def validate_contracts(
    data: dict[str, Any],
    domains: dict[str, dict[str, Any]],
    requirements: dict[str, dict[str, Any]],
    evidence: dict[str, dict[str, Any]],
    validation: Validation,
) -> dict[str, dict[str, Any]]:
    contracts = unique_index(
        object_list(data, "contracts", validation), "id", "contract", validation
    )
    allowed_kinds = {"api", "command", "query", "event", "job", "ai_candidate", "file"}
    allowed_statuses = {"draft", "specified", "implemented", "verified", "deprecated"}
    for contract_id, contract in contracts.items():
        if not VERSIONED_CONTRACT.search(contract_id):
            validation.error(
                f"Contract {contract_id} must end with a version such as @v1"
            )
        if contract.get("kind") not in allowed_kinds:
            validation.error(f"Contract {contract_id}.kind is invalid")
        if contract.get("owner") not in domains:
            validation.error(f"Contract {contract_id} references missing owner domain")
        if not non_empty_strings(contract.get("consumers")):
            validation.error(f"Contract {contract_id}.consumers must be non-empty")
        for field in ("schemaLocator", "authorization", "idempotency", "compatibility"):
            if not isinstance(contract.get(field), str) or not contract[field].strip():
                validation.error(f"Contract {contract_id}.{field} is required")
        if not isinstance(contract.get("schemaLocator"), str) or not contract[
            "schemaLocator"
        ].startswith("inline://contracts.json#/"):
            validation.error(
                f"Contract {contract_id}.schemaLocator must identify its inline frozen schema"
            )
        validate_json_schema(
            contract.get("inputSchema"),
            f"Contract {contract_id}.inputSchema",
            validation,
        )
        validate_json_schema(
            contract.get("outputSchema"),
            f"Contract {contract_id}.outputSchema",
            validation,
        )
        data_classes = contract.get("dataClasses")
        if not non_empty_strings(data_classes) or not set(data_classes).issubset(
            {"public", "internal", "personal", "sensitive"}
        ):
            validation.error(
                f"Contract {contract_id}.dataClasses must use declared classifications"
            )
        observability = contract.get("observability")
        if not isinstance(observability, dict):
            validation.error(f"Contract {contract_id}.observability must be an object")
        else:
            for field in ("correlation", "audit", "metrics"):
                if (
                    not isinstance(observability.get(field), str)
                    or not observability[field].strip()
                ):
                    validation.error(
                        f"Contract {contract_id}.observability.{field} is required"
                    )
        if not non_empty_strings(contract.get("errorSemantics")):
            validation.error(f"Contract {contract_id}.errorSemantics must be non-empty")
        requirement_refs = contract.get("requirementRefs", [])
        if not non_empty_strings(requirement_refs):
            validation.error(
                f"Contract {contract_id}.requirementRefs must be non-empty"
            )
        else:
            for ref in requirement_refs:
                if ref not in requirements:
                    validation.error(
                        f"Contract {contract_id} references missing requirement {ref}"
                    )
        refs = contract.get("evidenceRefs", [])
        if not optional_string_list(refs):
            validation.error(f"Contract {contract_id}.evidenceRefs must be an array")
            refs = []
        else:
            for ref in refs:
                if ref not in evidence:
                    validation.error(
                        f"Contract {contract_id} references missing evidence {ref}"
                    )
        status = contract.get("status")
        if status not in allowed_statuses:
            validation.error(f"Contract {contract_id}.status is invalid")
        elif status == "draft":
            validation.unresolved(
                f"Contract remains draft: {contract_id}", "contracts", contract_id
            )
        elif schema_contains_placeholder(
            contract.get("inputSchema")
        ) or schema_contains_placeholder(contract.get("outputSchema")):
            validation.error(
                f"Contract {contract_id} cannot be {status} while its schema contains draft placeholder language"
            )
        elif (
            isinstance(contract.get("draftBlocker"), str)
            and contract["draftBlocker"].strip()
        ):
            validation.error(
                f"Contract {contract_id} cannot be {status} while draftBlocker remains"
            )
        elif status == "implemented" and not evidence_supports(
            refs, evidence, allowed_classes={"code_observed"}, minimum_level=1
        ):
            validation.error(f"Contract {contract_id} implemented lacks code evidence")
        elif status == "verified" and not evidence_supports(
            refs, evidence, allowed_classes={"code_observed"}, minimum_level=2
        ):
            validation.error(
                f"Contract {contract_id} verified lacks executable test evidence"
            )
    if not contracts:
        validation.unresolved("No versioned contracts are defined", "contracts", "*")
    validate_domain_contract_references(domains, contracts, validation)
    return contracts


def validate_state_machines(
    data: dict[str, Any],
    domains: dict[str, dict[str, Any]],
    contracts: dict[str, dict[str, Any]],
    validation: Validation,
) -> dict[str, dict[str, Any]]:
    machines = unique_index(
        object_list(data, "machines", validation), "id", "state machine", validation
    )
    resource_owners = domain_resource_owners(domains)
    command_resources: dict[str, set[str]] = {}
    command_events: dict[str, set[str]] = {}

    def payload_field_literals(contract: dict[str, Any], field: str) -> set[str]:
        input_schema = contract.get("inputSchema")
        if not isinstance(input_schema, dict):
            return set()
        properties = input_schema.get("properties")
        if not isinstance(properties, dict):
            return set()
        payload = properties.get("payload")
        if not isinstance(payload, dict):
            return set()
        values: set[str] = set()

        def visit(node: Any) -> None:
            if isinstance(node, dict):
                node_properties = node.get("properties")
                if isinstance(node_properties, dict):
                    field_schema = node_properties.get(field)
                    if isinstance(field_schema, dict):
                        const_value = field_schema.get("const")
                        if isinstance(const_value, str) and const_value:
                            values.add(const_value)
                        enum_values = field_schema.get("enum")
                        if (
                            isinstance(enum_values, list)
                            and len(enum_values) == 1
                            and isinstance(enum_values[0], str)
                            and enum_values[0]
                        ):
                            values.add(enum_values[0])
                for child in node.values():
                    visit(child)
            elif isinstance(node, list):
                for child in node:
                    visit(child)

        visit(payload)
        return values

    for machine_id, machine in machines.items():
        owner = machine.get("owner")
        resource = machine.get("resource")
        if owner not in domains:
            validation.error(
                f"State machine {machine_id} references missing owner domain"
            )
        if not isinstance(resource, str) or not resource:
            validation.error(f"State machine {machine_id}.resource is required")
        elif resource_owners.get(resource) != owner:
            validation.error(
                f"State machine {machine_id} owner does not own canonical resource {resource}"
            )
        states = machine.get("states")
        if not non_empty_strings(states) or len(states) != len(set(states)):
            validation.error(
                f"State machine {machine_id}.states must contain unique states"
            )
            states = []
        state_set = set(states)
        if machine.get("initialState") not in state_set:
            validation.error(f"State machine {machine_id}.initialState is invalid")
        domain_commands = domains.get(owner, {}).get("commands", [])
        domain_events = domains.get(owner, {}).get("emits", [])
        if (
            isinstance(resource, str)
            and resource.endswith("release")
            and machine.get("initialState") in {"active", "published"}
            and any(
                "release-publish" in item
                for item in domain_commands
                if isinstance(item, str)
            )
        ):
            validation.error(
                f"State machine {machine_id} cannot start active when its domain defines a publish command"
            )
        terminals = machine.get("terminalStates", [])
        if not optional_string_list(terminals) or not set(terminals).issubset(
            state_set
        ):
            validation.error(f"State machine {machine_id}.terminalStates are invalid")
            terminals = []
        transitions = unique_index(
            object_list(machine, "transitions", validation),
            "id",
            f"state machine {machine_id} transition",
            validation,
        )
        outbound: set[str] = set()
        adjacency: dict[str, set[str]] = {state: set() for state in state_set}
        command_groups: dict[tuple[str, str], list[tuple[str, dict[str, Any]]]] = {}
        for transition_id, transition in transitions.items():
            source = transition.get("from")
            target = transition.get("to")
            command = transition.get("command")
            if source not in state_set or target not in state_set:
                validation.error(
                    f"Transition {machine_id}/{transition_id} references an invalid state"
                )
            else:
                outbound.add(source)
                adjacency[source].add(target)
            command_contract = contracts.get(command, {})
            if command not in contracts or command_contract.get("kind") != "command":
                validation.error(
                    f"Transition {machine_id}/{transition_id} requires a command contract"
                )
            else:
                participants = command_contract.get("crossDomainParticipants", [])
                participant_set = (
                    set(participants) if optional_string_list(participants) else set()
                )
                if command not in domain_commands and owner not in participant_set:
                    validation.error(
                        f"Transition {machine_id}/{transition_id} command {command} is not owned by domain {owner} and lacks an explicit cross-domain participant declaration"
                    )
                if (
                    command_contract.get("owner") != owner
                    and owner not in participant_set
                ):
                    validation.error(
                        f"Transition {machine_id}/{transition_id} command contract owner does not match state-machine owner {owner}"
                    )
            writes = transition.get("writesResources")
            if not non_empty_strings(writes):
                validation.error(
                    f"Transition {machine_id}/{transition_id}.writesResources must declare every canonical resource written"
                )
                writes = []
            elif isinstance(resource, str) and resource not in writes:
                validation.error(
                    f"Transition {machine_id}/{transition_id}.writesResources must include state-machine resource {resource}"
                )
            for written_resource in writes:
                if written_resource not in resource_owners:
                    validation.error(
                        f"Transition {machine_id}/{transition_id} writes unknown canonical resource {written_resource}"
                    )
            if command in contracts:
                command_resources.setdefault(str(command), set()).update(writes)
                command_events.setdefault(str(command), set())
            key = (str(source), str(command))
            command_groups.setdefault(key, []).append((transition_id, transition))
            for field in ("guards", "emits"):
                if not isinstance(transition.get(field), list):
                    validation.error(
                        f"Transition {machine_id}/{transition_id}.{field} must be an array"
                    )
            emitted = transition.get("emits", [])
            if not isinstance(emitted, list):
                emitted = []
            if command in contracts:
                command_events.setdefault(str(command), set()).update(
                    event for event in emitted if isinstance(event, str)
                )
            for event in emitted:
                if (
                    event not in contracts
                    or contracts.get(event, {}).get("kind") != "event"
                ):
                    validation.error(
                        f"Transition {machine_id}/{transition_id} emits missing event contract {event}"
                    )
                else:
                    if event not in domain_events:
                        validation.error(
                            f"Transition {machine_id}/{transition_id} emits event {event} not declared by owner domain {owner}"
                        )
                    if contracts[event].get("owner") != owner:
                        validation.error(
                            f"Transition {machine_id}/{transition_id} event contract {event} is not owned by state-machine owner {owner}"
                        )
            for field in ("idempotency", "concurrency"):
                if (
                    not isinstance(transition.get(field), str)
                    or not transition[field].strip()
                ):
                    validation.error(
                        f"Transition {machine_id}/{transition_id}.{field} is required"
                    )
        discriminator_records = machine.get("commandBranchDiscriminators", [])
        if not isinstance(discriminator_records, list):
            validation.error(
                f"State machine {machine_id}.commandBranchDiscriminators must be an array"
            )
            discriminator_records = []
        discriminator_index: dict[tuple[str, str], dict[str, Any]] = {}
        for record_index, record in enumerate(discriminator_records):
            if not isinstance(record, dict):
                validation.error(
                    f"State machine {machine_id}.commandBranchDiscriminators[{record_index}] must be an object"
                )
                continue
            source = record.get("from")
            command = record.get("command")
            key = (str(source), str(command))
            if (
                not isinstance(source, str)
                or not source
                or not isinstance(command, str)
                or not command
            ):
                validation.error(
                    f"State machine {machine_id}.commandBranchDiscriminators[{record_index}] requires from and command"
                )
                continue
            if key in discriminator_index:
                validation.error(
                    f"State machine {machine_id} has duplicate command branch discriminator for {command} from {source}"
                )
                continue
            discriminator_index[key] = record

        ambiguous_keys = {
            key
            for key, grouped_transitions in command_groups.items()
            if len(grouped_transitions) > 1
        }
        stale_keys = sorted(set(discriminator_index) - ambiguous_keys)
        for source, command in stale_keys:
            validation.error(
                f"State machine {machine_id} declares a stale command branch discriminator for {command} from {source}"
            )
        for (source, command), grouped_transitions in sorted(command_groups.items()):
            if len(grouped_transitions) == 1:
                transition_id, transition = grouped_transitions[0]
                if any(
                    field in transition
                    for field in ("discriminatorValue", "discriminatorContractEvidence")
                ):
                    validation.error(
                        f"Transition {machine_id}/{transition_id} declares discriminator metadata without an ambiguous command group"
                    )
                continue
            record = discriminator_index.get((source, command))
            if record is None:
                validation.error(
                    f"State machine {machine_id} has ambiguous command {command} from {source} without a closed branch discriminator"
                )
                continue
            field = record.get("field")
            if not isinstance(field, str) or not field:
                validation.error(
                    f"State machine {machine_id} command branch discriminator for {command} from {source} requires field"
                )
                continue
            if record.get("closed") is not True:
                validation.error(
                    f"State machine {machine_id} command branch discriminator for {command} from {source} must set closed=true"
                )
            branch_records = record.get("branches")
            if not isinstance(branch_records, list) or not branch_records:
                validation.error(
                    f"State machine {machine_id} command branch discriminator for {command} from {source} requires branches"
                )
                continue
            declared_branches: set[tuple[str, str, str]] = set()
            declared_values: list[str] = []
            for branch_index, branch in enumerate(branch_records):
                if not isinstance(branch, dict):
                    validation.error(
                        f"State machine {machine_id} command branch discriminator {command}/{source} branch {branch_index} must be an object"
                    )
                    continue
                value = branch.get("value")
                target = branch.get("to")
                transition_id = branch.get("transitionId")
                if not all(
                    isinstance(item, str) and item
                    for item in (value, target, transition_id)
                ):
                    validation.error(
                        f"State machine {machine_id} command branch discriminator {command}/{source} branch {branch_index} requires value, to and transitionId"
                    )
                    continue
                declared_values.append(value)
                declared_branches.add((transition_id, target, value))
            if len(declared_values) != len(set(declared_values)):
                validation.error(
                    f"State machine {machine_id} command branch discriminator for {command} from {source} has duplicate values"
                )
            expected_pairs = {
                (transition_id, str(transition.get("to")))
                for transition_id, transition in grouped_transitions
            }
            declared_pairs = {
                (transition_id, target)
                for transition_id, target, _ in declared_branches
            }
            if declared_pairs != expected_pairs or len(declared_branches) != len(
                grouped_transitions
            ):
                validation.error(
                    f"State machine {machine_id} command branch discriminator for {command} from {source} must cover every grouped transition exactly once"
                )
            contract = contracts.get(command, {})
            contract_values = payload_field_literals(contract, field)
            if set(declared_values) != contract_values:
                validation.error(
                    f"State machine {machine_id} command branch discriminator values for {command} from {source} must exactly match payload.{field} literals {sorted(contract_values)}"
                )
            branch_by_transition = {
                transition_id: value for transition_id, _, value in declared_branches
            }
            for transition_id, transition in grouped_transitions:
                expected_value = branch_by_transition.get(transition_id)
                if transition.get("discriminatorValue") != expected_value:
                    validation.error(
                        f"Transition {machine_id}/{transition_id}.discriminatorValue must equal its closed branch value"
                    )
                evidence = transition.get("discriminatorContractEvidence")
                expected_evidence = {
                    "contractId": command,
                    "schemaPath": f"payload.{field}",
                    "constValue": expected_value,
                }
                if evidence != expected_evidence:
                    validation.error(
                        f"Transition {machine_id}/{transition_id}.discriminatorContractEvidence must exactly bind {command} payload.{field}={expected_value}"
                    )
        for terminal in set(terminals) & outbound:
            validation.error(
                f"State machine {machine_id} terminal state {terminal} has an outbound transition"
            )
        initial = machine.get("initialState")
        if initial in state_set:
            reachable = {initial}
            frontier = [initial]
            while frontier:
                current = frontier.pop()
                for target in adjacency.get(current, set()):
                    if target not in reachable:
                        reachable.add(target)
                        frontier.append(target)
            unreachable = sorted(state_set - reachable)
            if unreachable:
                validation.error(
                    f"State machine {machine_id} has unreachable states from {initial}: {unreachable}"
                )
        if validation.mode != "draft":
            for state in state_set - set(terminals):
                if state not in outbound:
                    validation.error(
                        f"State machine {machine_id} non-terminal state {state} has no transition"
                    )
    for command, resources in sorted(command_resources.items()):
        contract = contracts.get(command, {})
        declared = contract.get("transactionScopeResources")
        boundary = contract.get("transactionBoundary")
        declared_events = contract.get("transactionEmits")
        if not non_empty_strings(declared):
            validation.unresolved(
                f"State-machine command {command} must declare transactionScopeResources exactly matching {sorted(resources)}",
                "contracts",
                command,
            )
        elif set(declared) != resources or len(declared) != len(resources):
            validation.error(
                f"Command {command}.transactionScopeResources must exactly match the state-machine writesResources union {sorted(resources)}"
            )
        if not isinstance(boundary, str) or not boundary.strip():
            validation.unresolved(
                f"State-machine command {command} must declare its atomic transactionBoundary",
                "contracts",
                command,
            )
        observed_events = command_events.get(command, set())
        if not optional_string_list(declared_events):
            validation.unresolved(
                f"State-machine command {command} must declare transactionEmits exactly matching {sorted(observed_events)}",
                "contracts",
                command,
            )
        elif set(declared_events) != observed_events or len(declared_events) != len(
            observed_events
        ):
            validation.error(
                f"Command {command}.transactionEmits must exactly match the state-machine emits union {sorted(observed_events)}"
            )
    for command, contract in contracts.items():
        if contract.get("kind") != "command":
            continue
        declared = contract.get("transactionScopeResources")
        if declared is None:
            continue
        if not non_empty_strings(declared):
            validation.error(
                f"Command {command}.transactionScopeResources must be a non-empty unique resource list when declared"
            )
            continue
        if len(declared) != len(set(declared)):
            validation.error(
                f"Command {command}.transactionScopeResources contains duplicate resources"
            )
        unknown = sorted(set(declared) - set(resource_owners))
        if unknown:
            validation.error(
                f"Command {command}.transactionScopeResources references unknown canonical resources {unknown}"
            )
        observed = command_resources.get(command)
        if observed and (set(declared) != observed or len(declared) != len(observed)):
            validation.error(
                f"Command {command}.transactionScopeResources must exactly match the state-machine writesResources union {sorted(observed)}"
            )
        if not observed:
            validation.unresolved(
                f"Command {command} declares transaction scope without state-machine writesResources evidence",
                "contracts",
                command,
            )
        declared_events = contract.get("transactionEmits")
        observed_events = command_events.get(command)
        if declared_events is not None:
            if not optional_string_list(declared_events):
                validation.error(
                    f"Command {command}.transactionEmits must be a unique event list when declared"
                )
            elif len(declared_events) != len(set(declared_events)):
                validation.error(
                    f"Command {command}.transactionEmits contains duplicate events"
                )
            elif (
                observed_events is not None and set(declared_events) != observed_events
            ):
                validation.error(
                    f"Command {command}.transactionEmits must exactly match the state-machine emits union {sorted(observed_events)}"
                )
            elif observed_events is None:
                validation.unresolved(
                    f"Command {command} declares transactionEmits without state-machine transition evidence",
                    "contracts",
                    command,
                )
    if not machines:
        validation.unresolved(
            "No canonical state machines are defined", "stateMachines", "*"
        )
    return machines


def state_is_specified(
    page_id: str,
    state_name: str,
    value: Any,
    decisions: dict[str, dict[str, Any]],
    validation: Validation,
) -> None:
    if not isinstance(value, dict):
        validation.unresolved(
            f"Page {page_id} has no behavior for state {state_name}", "pages", page_id
        )
        return
    if value.get("notApplicable") is True:
        if not accepted_decision(value.get("decisionRef"), decisions):
            validation.error(
                f"Page {page_id} state {state_name} is not applicable without an accepted decision"
            )
        return
    if not isinstance(value.get("behavior"), str) or not value["behavior"].strip():
        validation.unresolved(
            f"Page {page_id} state {state_name} lacks behavior", "pages", page_id
        )
    for field in ("visibleTruth", "freshness", "recovery", "correlation"):
        if not isinstance(value.get(field), str) or not value[field].strip():
            validation.unresolved(
                f"Page {page_id} state {state_name} lacks {field}", "pages", page_id
            )
    if not non_empty_strings(value.get("prohibitedClaims")):
        validation.unresolved(
            f"Page {page_id} state {state_name} lacks prohibitedClaims",
            "pages",
            page_id,
        )
    if not optional_string_list(value.get("allowedActions")):
        validation.unresolved(
            f"Page {page_id} state {state_name} lacks allowedActions", "pages", page_id
        )


def validate_pages(
    data: dict[str, Any],
    manifest: dict[str, Any],
    domains: dict[str, dict[str, Any]],
    decisions: dict[str, dict[str, Any]],
    validation: Validation,
) -> dict[str, dict[str, Any]]:
    profiles = unique_index(
        object_list(data, "stateProfiles", validation),
        "id",
        "state profile",
        validation,
    )
    for profile_id, profile in profiles.items():
        if not isinstance(profile.get("states"), dict):
            validation.error(f"State profile {profile_id}.states must be an object")
    pages = unique_index(
        object_list(data, "pages", validation), "id", "page", validation
    )
    routes: dict[str, str] = {}
    tracks = set(manifest.get("tracks", []))
    required_states = manifest.get("requiredPageStates", [])
    for page_id, page in pages.items():
        route = page.get("route")
        if not isinstance(route, str) or not route:
            validation.error(f"Page {page_id}.route is required")
        elif route in routes:
            validation.error(
                f"Route {route} is owned by both {routes[route]} and {page_id}"
            )
        else:
            routes[route] = page_id
        if not isinstance(page.get("purpose"), str) or not page["purpose"].strip():
            validation.error(f"Page {page_id}.purpose is required")
        scope = page.get("trackScope", [])
        if not non_empty_strings(scope) or not set(scope).issubset(tracks | {"shared"}):
            validation.error(f"Page {page_id}.trackScope contains undeclared tracks")
        data_owners = page.get("dataOwners")
        if not non_empty_strings(data_owners):
            validation.error(f"Page {page_id}.dataOwners must contain domain IDs")
            data_owners = []
        for owner in data_owners:
            if owner not in domains:
                validation.error(
                    f"Page {page_id} references missing data owner {owner}"
                )
        if validation.mode != "draft":
            for field in ("entryConditions", "actions", "exitRoutes", "permissions"):
                if not isinstance(page.get(field), list):
                    validation.error(f"Page {page_id}.{field} must be an array")
        profile_states: dict[str, Any] = {}
        profile_ref = page.get("stateProfileRef")
        if profile_ref is not None:
            if profile_ref not in profiles:
                validation.error(
                    f"Page {page_id} references missing state profile {profile_ref}"
                )
            elif isinstance(profiles[profile_ref].get("states"), dict):
                profile_states = profiles[profile_ref]["states"]
        states = page.get("states", {})
        if not isinstance(states, dict):
            validation.unresolved(
                f"Page {page_id}.states must be an object", "pages", page_id
            )
            states = {}
        resolved_states = {**profile_states, **states}
        for state_name in required_states:
            state_is_specified(
                page_id,
                state_name,
                resolved_states.get(state_name),
                decisions,
                validation,
            )
    if not pages:
        validation.unresolved("No pages are defined", "pages", "*")
    return pages


def validate_journeys(
    data: dict[str, Any],
    pages: dict[str, dict[str, Any]],
    requirements: dict[str, dict[str, Any]],
    validation: Validation,
) -> None:
    actors = unique_index(
        object_list(data, "actors", validation), "id", "actor", validation
    )
    for actor_id, actor in actors.items():
        if not isinstance(actor.get("name"), str) or not actor["name"].strip():
            validation.error(f"Actor {actor_id}.name is required")
        for field in ("permissions", "constraints"):
            if not isinstance(actor.get(field), list):
                validation.error(f"Actor {actor_id}.{field} must be an array")
    journeys = unique_index(
        object_list(data, "journeys", validation), "id", "journey", validation
    )
    for journey_id, journey in journeys.items():
        if journey.get("actorRef") not in actors:
            validation.error(f"Journey {journey_id} references missing actor")
        if (
            not isinstance(journey.get("outcome"), str)
            or not journey["outcome"].strip()
        ):
            validation.error(f"Journey {journey_id}.outcome is required")
        requirement_refs = journey.get("requirementRefs")
        if not non_empty_strings(requirement_refs):
            validation.error(f"Journey {journey_id}.requirementRefs must be non-empty")
        else:
            for ref in requirement_refs:
                if ref not in requirements:
                    validation.error(
                        f"Journey {journey_id} references missing requirement {ref}"
                    )
        steps = unique_index(
            object_list(journey, "steps", validation),
            "id",
            f"journey {journey_id} step",
            validation,
        )
        for step_id, step in steps.items():
            if step.get("surfaceRef") not in pages:
                validation.error(
                    f"Journey {journey_id}/{step_id} references missing page or workflow surface"
                )
            for field in ("action", "expectedState"):
                if not isinstance(step.get(field), str) or not step[field].strip():
                    validation.error(
                        f"Journey {journey_id}/{step_id}.{field} is required"
                    )
            if not isinstance(step.get("failurePaths"), list):
                validation.error(
                    f"Journey {journey_id}/{step_id}.failurePaths must be an array"
                )
        if not non_empty_strings(journey.get("acceptanceCommands")):
            validation.error(
                f"Journey {journey_id}.acceptanceCommands must be non-empty"
            )
    if not actors:
        validation.unresolved("No product actors are defined", "actors", "*")
    if not journeys:
        validation.unresolved("No end-to-end journeys are defined", "journeys", "*")


def validate_data_lifecycle(
    data: dict[str, Any],
    domains: dict[str, dict[str, Any]],
    evidence: dict[str, dict[str, Any]],
    decisions: dict[str, dict[str, Any]],
    validation: Validation,
) -> None:
    resources = unique_index(
        object_list(data, "resources", validation), "id", "data resource", validation
    )
    owners = domain_resource_owners(domains)
    allowed_classifications = {"public", "internal", "personal", "sensitive"}
    allowed_states = {"confirmed", "proposed", "unknown", "not_applicable"}
    for resource_id, resource in resources.items():
        if owners.get(resource_id) != resource.get("owner"):
            validation.error(
                f"Data resource {resource_id} owner does not match its canonical domain owner"
            )
        if resource.get("classification") not in allowed_classifications:
            validation.error(f"Data resource {resource_id}.classification is invalid")
        if resource.get("state") not in allowed_states:
            validation.error(f"Data resource {resource_id}.state is invalid")
        if resource.get("state") in {"unknown", "proposed"}:
            validation.unresolved(
                f"Data lifecycle remains unknown: {resource_id}",
                "dataLifecycleResources",
                resource_id,
            )
        if resource.get("state") == "confirmed" and not evidence_supports(
            resource.get("evidenceRefs", []),
            evidence,
            allowed_classes={"user_confirmed", "external_verified"},
            minimum_level=0,
        ):
            validation.error(
                f"Confirmed data lifecycle {resource_id} requires user-confirmed or external evidence"
            )
        if resource.get("state") == "not_applicable" and not accepted_decision(
            resource.get("decisionRef"), decisions
        ):
            validation.error(
                f"Data resource {resource_id} not_applicable requires an accepted decision"
            )
        for field in (
            "purpose",
            "source",
            "retention",
            "deletion",
            "export",
            "encryption",
            "residency",
        ):
            if not isinstance(resource.get(field), str) or not resource[field].strip():
                validation.error(f"Data resource {resource_id}.{field} is required")
        refs = resource.get("evidenceRefs", [])
        if not optional_string_list(refs):
            validation.error(
                f"Data resource {resource_id}.evidenceRefs must be an array"
            )
            refs = []
        for ref in refs:
            if ref not in evidence:
                validation.error(
                    f"Data resource {resource_id} references missing evidence {ref}"
                )
        derived = resource.get("derivedFrom", [])
        if not optional_string_list(derived):
            validation.error(
                f"Data resource {resource_id}.derivedFrom must be an array"
            )
        for field in (
            "dataSubjects",
            "dataElements",
            "processingPurposes",
            "vendorRefs",
            "deletionPropagation",
            "lineageKeys",
            "blockingReasons",
        ):
            if not isinstance(resource.get(field), list) or not all(
                isinstance(item, str) and item for item in resource.get(field, [])
            ):
                validation.error(
                    f"Data resource {resource_id}.{field} must be an array"
                )
        if resource.get("state") == "unknown" and not non_empty_strings(
            resource.get("blockingReasons")
        ):
            validation.error(
                f"Unknown data lifecycle {resource_id} requires blockingReasons"
            )
        if resource.get("legalBasisState") not in {
            "confirmed",
            "proposed",
            "unknown",
            "not_applicable",
        }:
            validation.error(f"Data resource {resource_id}.legalBasisState is invalid")
        if resource.get("visibility") not in {
            "account_private",
            "authorized_shared",
            "public_projection",
            "internal_restricted",
        }:
            validation.error(f"Data resource {resource_id}.visibility is invalid")
        if resource.get("modelTrainingUse") not in {
            "prohibited",
            "separate_explicit_opt_in",
            "not_applicable",
            "unknown",
        }:
            validation.error(f"Data resource {resource_id}.modelTrainingUse is invalid")
        if not isinstance(resource.get("classificationByState"), dict):
            validation.error(
                f"Data resource {resource_id}.classificationByState must be an object"
            )
        for field in (
            "minorPolicyRef",
            "backupDisposition",
            "legalHoldSemantics",
            "correctionSemantics",
        ):
            if not isinstance(resource.get(field), str) or not resource[field].strip():
                validation.error(f"Data resource {resource_id}.{field} is required")
    for resource_id, resource in resources.items():
        derived = resource.get("derivedFrom", [])
        if not optional_string_list(derived):
            derived = []
        for source in derived:
            if source not in resources:
                validation.error(
                    f"Data resource {resource_id} derives from missing resource {source}"
                )
    if validation.mode != "draft":
        for owned_resource in owners:
            if owned_resource not in resources:
                validation.error(
                    f"Canonical resource {owned_resource} has no data lifecycle declaration"
                )
    if not resources:
        validation.unresolved(
            "No data lifecycle resources are defined", "dataLifecycleResources", "*"
        )


def validate_ai_scenarios(
    data: dict[str, Any],
    domains: dict[str, dict[str, Any]],
    contracts: dict[str, dict[str, Any]],
    evidence: dict[str, dict[str, Any]],
    decisions: dict[str, dict[str, Any]],
    validation: Validation,
) -> None:
    scenarios = unique_index(
        object_list(data, "scenarios", validation), "id", "AI scenario", validation
    )
    allowed_statuses = {"draft", "specified", "implemented", "verified", "retired"}
    for scenario_id, scenario in scenarios.items():
        if scenario.get("owner") not in domains:
            validation.error(
                f"AI scenario {scenario_id} references missing owner domain"
            )
        for field in ("businessOwner", "factOwner"):
            if scenario.get(field) not in domains:
                validation.error(
                    f"AI scenario {scenario_id}.{field} references missing domain"
                )
        if (
            not isinstance(scenario.get("purpose"), str)
            or not scenario["purpose"].strip()
        ):
            validation.error(f"AI scenario {scenario_id}.purpose is required")
        inputs = scenario.get("inputContracts")
        if not non_empty_strings(inputs):
            validation.error(
                f"AI scenario {scenario_id}.inputContracts must be non-empty"
            )
            inputs = []
        for contract_id in inputs:
            if contract_id not in contracts:
                validation.error(
                    f"AI scenario {scenario_id} references missing input contract {contract_id}"
                )
        output = scenario.get("outputContract")
        if (
            output not in contracts
            or contracts.get(output, {}).get("kind") != "ai_candidate"
        ):
            validation.error(
                f"AI scenario {scenario_id} requires an AI-candidate output contract"
            )
        activation = scenario.get("activationState")
        if activation not in {"blocked", "eligible", "active", "retired"}:
            validation.error(f"AI scenario {scenario_id}.activationState is invalid")
        blocking_decisions = scenario.get("blockingDecisionRefs", [])
        if not isinstance(blocking_decisions, list) or not all(
            isinstance(item, str) and item for item in blocking_decisions
        ):
            validation.error(
                f"AI scenario {scenario_id}.blockingDecisionRefs must be an array"
            )
            blocking_decisions = []
        for decision_ref in blocking_decisions:
            if decision_ref not in decisions:
                validation.error(
                    f"AI scenario {scenario_id} references missing decision {decision_ref}"
                )
        if scenario.get("candidateOnly") is not True:
            if not accepted_decision(
                scenario.get("factAuthorityDecisionRef"), decisions
            ):
                validation.error(
                    f"AI scenario {scenario_id} may not own facts without an accepted decision"
                )
        confirmation = scenario.get("confirmation")
        if confirmation not in {"user", "deterministic", "both", "not_required"}:
            validation.error(f"AI scenario {scenario_id}.confirmation is invalid")
        elif confirmation == "not_required" and not accepted_decision(
            scenario.get("confirmationDecisionRef"), decisions
        ):
            validation.error(
                f"AI scenario {scenario_id} not_required confirmation needs an accepted decision"
            )
        retrieval = scenario.get("retrieval")
        if not isinstance(retrieval, dict):
            validation.error(f"AI scenario {scenario_id}.retrieval must be an object")
        else:
            for field in ("scope", "citationPolicy", "injectionPolicy"):
                if (
                    not isinstance(retrieval.get(field), str)
                    or not retrieval[field].strip()
                ):
                    validation.error(
                        f"AI scenario {scenario_id}.retrieval.{field} is required"
                    )
        for field in (
            "promptVersion",
            "modelPolicyRef",
            "budgetPolicyRef",
            "evalSuiteLocator",
            "failureSemantics",
        ):
            if not isinstance(scenario.get(field), str) or not scenario[field].strip():
                validation.error(f"AI scenario {scenario_id}.{field} is required")
        for field in ("prohibitedClaims", "abstentionConditions", "citationValidators"):
            if not non_empty_strings(scenario.get(field)):
                validation.error(f"AI scenario {scenario_id}.{field} must be non-empty")
        if scenario.get("groundingRequired") is not True:
            validation.error(
                f"AI scenario {scenario_id}.groundingRequired must be true"
            )
        for field in (
            "confidenceSemantics",
            "independentReviewPolicy",
            "vendorProcessingPolicy",
            "cachePolicy",
            "reservationPolicy",
            "failureRefundPolicy",
            "minorHandling",
            "exceptionEscalation",
        ):
            if not isinstance(scenario.get(field), str) or not scenario[field].strip():
                validation.error(f"AI scenario {scenario_id}.{field} is required")
        if not isinstance(scenario.get("toolPermissions"), list) or not all(
            isinstance(item, str) and item
            for item in scenario.get("toolPermissions", [])
        ):
            validation.error(
                f"AI scenario {scenario_id}.toolPermissions must be an array"
            )
        attempts = scenario.get("maxAttempts")
        if (
            not isinstance(attempts, int)
            or isinstance(attempts, bool)
            or not 1 <= attempts <= 5
        ):
            validation.error(f"AI scenario {scenario_id}.maxAttempts must be 1..5")
        if scenario.get("noHumanSuccessPath") is not True:
            validation.error(
                f"AI scenario {scenario_id}.noHumanSuccessPath must be true"
            )
        quality = scenario.get("qualityGate")
        if not isinstance(quality, dict):
            validation.error(f"AI scenario {scenario_id}.qualityGate must be an object")
        else:
            quality_status = quality.get("status")
            if quality_status not in {"unknown", "proposed", "confirmed"}:
                validation.error(
                    f"AI scenario {scenario_id}.qualityGate.status is invalid"
                )
            elif quality_status in {"unknown", "proposed"}:
                validation.unresolved(
                    f"AI scenario {scenario_id} quality gate remains {quality_status}",
                    "aiScenarios",
                    scenario_id,
                )
            if not non_empty_strings(quality.get("metrics")):
                validation.error(
                    f"AI scenario {scenario_id}.qualityGate.metrics is required"
                )
            if not non_empty_strings(quality.get("thresholds")):
                validation.error(
                    f"AI scenario {scenario_id}.qualityGate.thresholds is required"
                )
            for field in ("evalDatasetRef", "reviewCadence"):
                if (
                    not isinstance(quality.get(field), str)
                    or not quality[field].strip()
                ):
                    validation.error(
                        f"AI scenario {scenario_id}.qualityGate.{field} is required"
                    )
            quality_refs = quality.get("evidenceRefs", [])
            if not isinstance(quality_refs, list) or not all(
                isinstance(item, str) and item for item in quality_refs
            ):
                validation.error(
                    f"AI scenario {scenario_id}.qualityGate.evidenceRefs must be an array"
                )
                quality_refs = []
            for ref in quality_refs:
                if ref not in evidence:
                    validation.error(
                        f"AI scenario {scenario_id} quality gate references missing evidence {ref}"
                    )
            if quality_status == "confirmed" and not evidence_supports(
                quality_refs,
                evidence,
                allowed_classes={"user_confirmed", "external_verified"},
                minimum_level=0,
            ):
                validation.error(
                    f"AI scenario {scenario_id} confirmed quality gate lacks authorized evidence"
                )
        if not non_empty_strings(scenario.get("dataClasses")):
            validation.error(f"AI scenario {scenario_id}.dataClasses must be non-empty")
        refs = scenario.get("evidenceRefs", [])
        if not optional_string_list(refs):
            validation.error(f"AI scenario {scenario_id}.evidenceRefs must be an array")
            refs = []
        for ref in refs:
            if ref not in evidence:
                validation.error(
                    f"AI scenario {scenario_id} references missing evidence {ref}"
                )
        status = scenario.get("status")
        if status not in allowed_statuses:
            validation.error(f"AI scenario {scenario_id}.status is invalid")
        elif status == "draft":
            validation.unresolved(
                f"AI scenario remains draft: {scenario_id}", "aiScenarios", scenario_id
            )
        elif status == "implemented" and not evidence_supports(
            refs, evidence, allowed_classes={"code_observed"}, minimum_level=1
        ):
            validation.error(
                f"AI scenario {scenario_id} implemented lacks code evidence"
            )
        elif status == "verified" and not evidence_supports(
            refs, evidence, allowed_classes={"code_observed"}, minimum_level=4
        ):
            validation.error(
                f"AI scenario {scenario_id} verified lacks integration evidence"
            )
        if activation in {"eligible", "active"}:
            referenced = [*inputs, output]
            if any(
                contracts.get(ref, {}).get("status") == "draft" for ref in referenced
            ):
                validation.error(
                    f"AI scenario {scenario_id} cannot be {activation} with draft contracts"
                )
            if not isinstance(quality, dict) or quality.get("status") != "confirmed":
                validation.error(
                    f"AI scenario {scenario_id} cannot be {activation} without a confirmed quality gate"
                )
            if any(not accepted_decision(ref, decisions) for ref in blocking_decisions):
                validation.error(
                    f"AI scenario {scenario_id} cannot be {activation} with unresolved decisions"
                )
        if activation == "active" and status != "verified":
            validation.error(
                f"AI scenario {scenario_id} active requires verified status"
            )
    if not scenarios:
        validation.unresolved("No AI scenarios are defined", "aiScenarios", "*")


def validate_entitlements(
    data: dict[str, Any],
    domains: dict[str, dict[str, Any]],
    requirements: dict[str, dict[str, Any]],
    decisions: dict[str, dict[str, Any]],
    contracts: dict[str, dict[str, Any]],
    validation: Validation,
) -> None:
    capabilities = unique_index(
        object_list(data, "capabilities", validation),
        "id",
        "entitlement capability",
        validation,
    )
    for capability_id, capability in capabilities.items():
        if capability.get("owner") not in domains:
            validation.error(
                f"Entitlement {capability_id} references missing owner domain"
            )
        if capability.get("businessOwner") not in domains:
            validation.error(
                f"Entitlement {capability_id}.businessOwner references missing domain"
            )
        if capability.get("requirementRef") not in requirements:
            validation.error(
                f"Entitlement {capability_id} references missing requirement"
            )
        access = capability.get("access")
        if access not in {"free", "paid", "metered", "unknown", "not_applicable"}:
            validation.error(f"Entitlement {capability_id}.access is invalid")
        elif access == "unknown":
            validation.unresolved(
                f"Entitlement boundary remains unknown: {capability_id}",
                "entitlements",
                capability_id,
            )
            blocking_ref = capability.get("blockingDecisionRef")
            if not isinstance(blocking_ref, str) or blocking_ref not in decisions:
                validation.error(
                    f"Unknown entitlement {capability_id} requires a blocking decision"
                )
        elif access in {
            "free",
            "paid",
            "metered",
            "not_applicable",
        } and not accepted_decision(capability.get("decisionRef"), decisions):
            validation.error(
                f"Entitlement {capability_id} {access} requires an accepted decision"
            )
        if access == "metered" and (
            not isinstance(capability.get("unit"), str)
            or not capability["unit"].strip()
        ):
            validation.error(
                f"Entitlement {capability_id}.unit is required for metering"
            )
        if not non_empty_strings(capability.get("lifecycleStates")):
            validation.error(
                f"Entitlement {capability_id}.lifecycleStates must be non-empty"
            )
        else:
            required_states = {"unresolved", "available", "suspended", "revoked"}
            if not required_states.issubset(set(capability["lifecycleStates"])):
                validation.error(
                    f"Entitlement {capability_id}.lifecycleStates lacks commercial states"
                )
        activation = capability.get("activationState")
        if activation not in {"blocked", "active", "retired"}:
            validation.error(f"Entitlement {capability_id}.activationState is invalid")
        if access == "unknown" and activation != "blocked":
            validation.error(f"Unknown entitlement {capability_id} must remain blocked")
        if activation == "active" and access == "unknown":
            validation.error(
                f"Entitlement {capability_id} active cannot have unknown access"
            )
        if capability.get("platformPolicyState") not in {
            "unknown",
            "proposed",
            "confirmed",
        }:
            validation.error(
                f"Entitlement {capability_id}.platformPolicyState is invalid"
            )
        elif capability.get("platformPolicyState") in {"unknown", "proposed"}:
            validation.unresolved(
                f"Entitlement {capability_id} platform policy remains unresolved",
                "entitlements",
                capability_id,
            )
        for field in (
            "qualityFloor",
            "reservationStateMachineRef",
            "refundSemantics",
            "contentLicenseDependency",
            "measurementSource",
            "blockingReason",
        ):
            if (
                not isinstance(capability.get(field), str)
                or not capability[field].strip()
            ):
                validation.error(f"Entitlement {capability_id}.{field} is required")
        contract_refs = capability.get("contractRefs")
        if not non_empty_strings(contract_refs):
            validation.error(
                f"Entitlement {capability_id}.contractRefs must be non-empty"
            )
        else:
            for contract_ref in contract_refs:
                if contract_ref not in contracts:
                    validation.error(
                        f"Entitlement {capability_id} references missing contract {contract_ref}"
                    )
    if not capabilities:
        validation.unresolved(
            "No entitlement capabilities are defined", "entitlements", "*"
        )


def validate_coverage(
    data: dict[str, Any],
    manifest: dict[str, Any],
    domains: dict[str, dict[str, Any]],
    contracts: dict[str, dict[str, Any]],
    decisions: dict[str, dict[str, Any]],
    evidence: dict[str, dict[str, Any]],
    validation: Validation,
) -> None:
    capabilities = unique_index(
        object_list(data, "capabilities", validation),
        "capability",
        "capability",
        validation,
    )
    allowed_statuses = {
        "unknown",
        "draft",
        "specified",
        "implemented",
        "verified",
        "unavailable",
        "not_applicable",
    }
    tracks = manifest.get("tracks", [])
    for capability in manifest.get("dualTrackCapabilities", []):
        entry = capabilities.get(capability)
        if entry is None:
            validation.unresolved(
                f"Missing track capability: {capability}",
                "trackCoverage",
                f"{capability}::*",
            )
            continue
        coverage = entry.get("tracks")
        if not isinstance(coverage, dict):
            validation.error(f"Capability {capability}.tracks must be an object")
            continue
        for track in tracks:
            item = coverage.get(track)
            if not isinstance(item, dict):
                validation.unresolved(
                    f"Capability {capability} lacks track {track}",
                    "trackCoverage",
                    f"{capability}::{track}",
                )
                continue
            status = item.get("status")
            if status not in allowed_statuses:
                validation.error(
                    f"Capability {capability}/{track} has an invalid status"
                )
                continue
            if status == "unknown":
                validation.unresolved(
                    f"Capability {capability}/{track} remains unknown",
                    "trackCoverage",
                    f"{capability}::{track}",
                )
            elif status in {"unavailable", "not_applicable"}:
                if not accepted_decision(item.get("decisionRef"), decisions):
                    validation.error(
                        f"Capability {capability}/{track} {status} requires an accepted decision"
                    )
            else:
                if status == "draft":
                    validation.unresolved(
                        f"Capability {capability}/{track} remains draft",
                        "trackCoverage",
                        f"{capability}::{track}",
                    )
                producer = item.get("producer")
                if producer not in domains:
                    validation.error(
                        f"Capability {capability}/{track} has no canonical producer"
                    )
                if (
                    not isinstance(item.get("contract"), str)
                    or not item["contract"].strip()
                ):
                    validation.error(
                        f"Capability {capability}/{track} has no versioned contract"
                    )
                elif not VERSIONED_CONTRACT.search(item["contract"]):
                    validation.error(
                        f"Capability {capability}/{track} contract must end with a version such as @v1"
                    )
                elif item["contract"] not in contracts:
                    validation.error(
                        f"Capability {capability}/{track} references missing contract {item['contract']}"
                    )
                elif status == "specified" and contracts[item["contract"]].get(
                    "status"
                ) not in {
                    "specified",
                    "implemented",
                    "verified",
                }:
                    validation.error(
                        f"Capability {capability}/{track} specified requires a specified contract"
                    )
                elif status == "implemented" and contracts[item["contract"]].get(
                    "status"
                ) not in {
                    "implemented",
                    "verified",
                }:
                    validation.error(
                        f"Capability {capability}/{track} implemented requires an implemented contract"
                    )
                elif (
                    status == "verified"
                    and contracts[item["contract"]].get("status") != "verified"
                ):
                    validation.error(
                        f"Capability {capability}/{track} verified requires a verified contract"
                    )
                if not optional_string_list(item.get("consumers")):
                    validation.error(
                        f"Capability {capability}/{track}.consumers must be an array"
                    )
                refs = item.get("evidenceRefs", [])
                if not optional_string_list(refs):
                    validation.error(
                        f"Capability {capability}/{track}.evidenceRefs must be an array"
                    )
                    refs = []
                else:
                    for ref in refs:
                        if ref not in evidence:
                            validation.error(
                                f"Capability {capability}/{track} references missing evidence {ref}"
                            )
                if (
                    status == "specified"
                    and not refs
                    and not accepted_decision(item.get("decisionRef"), decisions)
                ):
                    validation.error(
                        f"Capability {capability}/{track} specified requires evidence or an accepted decision"
                    )
                if status in {"implemented", "verified"}:
                    if not non_empty_strings(refs):
                        validation.error(
                            f"Capability {capability}/{track} {status} lacks evidence"
                        )
                    minimum_level = 1 if status == "implemented" else 3
                    if not evidence_supports(
                        refs,
                        evidence,
                        allowed_classes={"code_observed"},
                        minimum_level=minimum_level,
                    ):
                        validation.error(
                            f"Capability {capability}/{track} {status} lacks proportional code evidence"
                        )


def path_base(raw: str) -> PurePosixPath:
    normalized = raw.replace("\\", "/").strip()
    while normalized.startswith("./"):
        normalized = normalized[2:]
    for marker in ("/**", "/*"):
        if normalized.endswith(marker):
            normalized = normalized[: -len(marker)]
    return PurePosixPath(normalized or ".")


def safe_relative_pattern(raw: str) -> bool:
    if not isinstance(raw, str) or not raw.strip():
        return False
    normalized = raw.replace("\\", "/").strip()
    candidate = PurePosixPath(normalized)
    if candidate.is_absolute() or ".." in candidate.parts:
        return False
    wildcard_tokens = ("*", "?", "[", "]")
    if any(token in normalized for token in wildcard_tokens):
        if not normalized.endswith(("/**", "/*")):
            return False
        base = normalized.rsplit("/", 1)[0]
        if any(token in base for token in wildcard_tokens):
            return False
    return True


def safe_relative_file(raw: str) -> bool:
    return safe_relative_pattern(raw) and not any(
        token in raw for token in ("*", "?", "[", "]")
    )


def paths_overlap(left: str, right: str) -> bool:
    left_parts = path_base(left).parts
    right_parts = path_base(right).parts
    limit = min(len(left_parts), len(right_parts))
    return left_parts[:limit] == right_parts[:limit]


def path_is_covered_by_pattern(child: str, parent: str) -> bool:
    """Return whether every path selected by ``child`` is owned by ``parent``."""

    child_parts = path_base(child).parts
    parent_parts = path_base(parent).parts
    return (
        len(child_parts) >= len(parent_parts)
        and child_parts[: len(parent_parts)] == parent_parts
    )


def dependency_cycle(packages: dict[str, dict[str, Any]]) -> list[str] | None:
    visiting: set[str] = set()
    visited: set[str] = set()

    def visit(package_id: str, trail: list[str]) -> list[str] | None:
        if package_id in visiting:
            return trail[trail.index(package_id) :] + [package_id]
        if package_id in visited:
            return None
        visiting.add(package_id)
        trail.append(package_id)
        dependencies = packages[package_id].get("dependsOn", [])
        if not optional_string_list(dependencies):
            dependencies = []
        for dependency in dependencies:
            if dependency in packages:
                found = visit(dependency, trail)
                if found:
                    return found
        trail.pop()
        visiting.remove(package_id)
        visited.add(package_id)
        return None

    for package_id in packages:
        found = visit(package_id, [])
        if found:
            return found
    return None


def package_reaches(
    source: str,
    target: str,
    packages: dict[str, dict[str, Any]],
    seen: set[str] | None = None,
) -> bool:
    visited = seen if seen is not None else set()
    if source in visited:
        return False
    visited.add(source)
    dependencies = packages[source].get("dependsOn", [])
    if not optional_string_list(dependencies):
        return False
    for dependency in dependencies:
        if dependency == target:
            return True
        if dependency in packages and package_reaches(
            dependency, target, packages, visited
        ):
            return True
    return False


def validate_acceptance_commands(
    package_id: str, value: Any, validation: Validation
) -> None:
    if not isinstance(value, list) or not value:
        validation.error(
            f"Work package {package_id}.acceptanceCommands must be non-empty"
        )
        return
    command_ids: set[str] = set()
    for index, command in enumerate(value):
        if not isinstance(command, dict):
            validation.error(
                f"Work package {package_id}.acceptanceCommands[{index}] must be an object"
            )
            continue
        command_id = command.get("id")
        if not isinstance(command_id, str) or not command_id:
            validation.error(
                f"Work package {package_id}.acceptanceCommands[{index}].id is required"
            )
        elif command_id in command_ids:
            validation.error(
                f"Work package {package_id} repeats command ID {command_id}"
            )
        else:
            command_ids.add(command_id)
        kind = command.get("kind")
        if kind == "scenario_test":
            for issue in validate_scenario_command(command):
                validation.error(f"Work package {package_id}/{command_id}: {issue}")
        elif kind == "check":
            expected_fields = {
                "id",
                "kind",
                "argv",
                "cwd",
                "timeoutSeconds",
                "expectedExitCode",
            }
            if set(command) != expected_fields:
                validation.error(
                    f"Work package {package_id}/{command_id} check command fields must be {sorted(expected_fields)}"
                )
            if not non_empty_strings(command.get("argv")):
                validation.error(
                    f"Work package {package_id}/{command_id}.argv must be non-empty"
                )
            cwd = command.get("cwd", ".")
            if not safe_relative_file(cwd):
                validation.error(
                    f"Work package {package_id}/{command_id}.cwd is unsafe"
                )
            timeout = command.get("timeoutSeconds")
            if (
                not isinstance(timeout, int)
                or isinstance(timeout, bool)
                or not 1 <= timeout <= 3600
            ):
                validation.error(
                    f"Work package {package_id}/{command_id}.timeoutSeconds must be 1..3600"
                )
            expected_exit = command.get("expectedExitCode")
            if not isinstance(expected_exit, int) or isinstance(expected_exit, bool):
                validation.error(
                    f"Work package {package_id}/{command_id}.expectedExitCode must be an integer"
                )
            elif expected_exit != 0:
                validation.error(
                    f"Work package {package_id}/{command_id}.expectedExitCode must be zero; negative behavior belongs inside a passing test assertion"
                )
        else:
            validation.error(
                f"Work package {package_id}/{command_id}.kind must be check or scenario_test"
            )


def acceptance_command_executes_test_selector(command: Any, selector: str) -> bool:
    return (
        isinstance(command, dict)
        and command.get("kind") == "scenario_test"
        and command.get("selector") == selector
        and not validate_scenario_command(command)
    )


def validate_work_packages(
    data: dict[str, Any],
    contracts: dict[str, dict[str, Any]],
    requirements: dict[str, dict[str, Any]],
    decisions: dict[str, dict[str, Any]],
    validation: Validation,
    *,
    frozen_package_ids: set[str] | None = None,
) -> dict[str, dict[str, Any]]:
    packages = unique_index(
        object_list(data, "packages", validation), "id", "work package", validation
    )
    frozen_package_ids = frozen_package_ids or set()
    runnable: list[tuple[str, str]] = []
    for package_id, package in packages.items():
        status = package.get("status")
        if status not in WORK_PACKAGE_STATUSES:
            validation.error(f"Work package {package_id}.status is invalid")
        if package.get("wave") not in WORK_PACKAGE_WAVES:
            validation.error(f"Work package {package_id}.wave is invalid")
        if (
            not isinstance(package.get("objective"), str)
            or not package["objective"].strip()
        ):
            validation.error(f"Work package {package_id}.objective is required")
        if not isinstance(package.get("owner"), str) or not package["owner"].strip():
            validation.error(f"Work package {package_id}.owner is required")
        if package.get("riskClass") not in {"low", "medium", "high", "critical"}:
            validation.error(f"Work package {package_id}.riskClass is invalid")
        if not non_empty_strings(package.get("deliverables")):
            validation.error(
                f"Work package {package_id}.deliverables must be non-empty"
            )
        requirement_refs = package.get("requirementRefs")
        if not non_empty_strings(requirement_refs):
            validation.error(
                f"Work package {package_id}.requirementRefs must be non-empty"
            )
        else:
            for requirement_ref in requirement_refs:
                if requirement_ref not in requirements:
                    validation.error(
                        f"Work package {package_id} references missing requirement {requirement_ref}"
                    )
        decision_refs = package.get("decisionRefs")
        if not non_empty_strings(decision_refs):
            validation.error(
                f"Work package {package_id}.decisionRefs must be non-empty"
            )
        else:
            for decision_ref in decision_refs:
                if not accepted_decision(decision_ref, decisions):
                    validation.error(
                        f"Work package {package_id} requires accepted decision {decision_ref}"
                    )
        blocking_refs = package.get("blockingDecisionRefs", [])
        if not optional_string_list(blocking_refs):
            validation.error(
                f"Work package {package_id}.blockingDecisionRefs must be an array"
            )
            blocking_refs = []
        for decision_ref in blocking_refs:
            if decision_ref not in decisions:
                validation.error(
                    f"Work package {package_id} references missing blocking decision {decision_ref}"
                )
            elif not accepted_decision(decision_ref, decisions):
                validation.unresolved(
                    f"Work package {package_id} is blocked by decision {decision_ref}",
                    "workPackages",
                    package_id,
                )
        if not optional_string_list(package.get("nonGoals")):
            validation.error(f"Work package {package_id}.nonGoals must be an array")
        draft_contract_refs: list[str] = []
        for field in ("inputContracts", "outputContracts"):
            if not optional_string_list(package.get(field)):
                validation.error(f"Work package {package_id}.{field} must be an array")
            else:
                for contract_id in package.get(field, []):
                    if contract_id not in contracts:
                        validation.error(
                            f"Work package {package_id}.{field} references missing contract {contract_id}"
                        )
                    elif contracts[contract_id].get("status") == "draft":
                        draft_contract_refs.append(f"{field}:{contract_id}")
        if draft_contract_refs:
            preview = ", ".join(draft_contract_refs[:5])
            remainder = len(draft_contract_refs) - 5
            suffix = f" (+{remainder} more)" if remainder > 0 else ""
            validation.unresolved(
                f"Work package {package_id} references {len(draft_contract_refs)} draft contracts: "
                f"{preview}{suffix}",
                "workPackages",
                package_id,
            )
        if not package.get("inputContracts") and not package.get("outputContracts"):
            validation.error(
                f"Work package {package_id} must reference at least one input or output contract"
            )
        dependencies = package.get("dependsOn", [])
        if not optional_string_list(dependencies):
            validation.error(f"Work package {package_id}.dependsOn must be an array")
            dependencies = []
        else:
            for dependency in dependencies:
                if dependency not in packages:
                    validation.error(
                        f"Work package {package_id} has missing dependency {dependency}"
                    )
        allowed_paths = package.get("allowedPaths")
        if not non_empty_strings(allowed_paths):
            validation.error(
                f"Work package {package_id}.allowedPaths must be non-empty"
            )
        else:
            for path in allowed_paths:
                if not safe_relative_pattern(path):
                    validation.error(
                        f"Work package {package_id} has unsafe allowed path: {path}"
                    )
            if status == "planned" and package_id not in frozen_package_ids:
                runnable.extend((package_id, path) for path in allowed_paths)
        forbidden_paths = package.get("forbiddenPaths")
        if not non_empty_strings(forbidden_paths):
            validation.error(
                f"Work package {package_id}.forbiddenPaths must be non-empty"
            )
            forbidden_paths = []
        else:
            for path in forbidden_paths:
                if not safe_relative_pattern(path):
                    validation.error(
                        f"Work package {package_id} has unsafe forbidden path: {path}"
                    )
            if isinstance(allowed_paths, list):
                for allowed in allowed_paths:
                    for forbidden in forbidden_paths:
                        if paths_overlap(allowed, forbidden):
                            validation.error(
                                f"Work package {package_id} allowed/forbidden paths overlap: "
                                f"{allowed} and {forbidden}"
                            )
        migration = package.get("migrationPolicy")
        if not isinstance(migration, dict):
            validation.error(
                f"Work package {package_id}.migrationPolicy must be an object"
            )
        else:
            if migration.get("mode") not in {"none", "owner", "consumer"}:
                validation.error(
                    f"Work package {package_id}.migrationPolicy.mode is invalid"
                )
            migration_paths = migration.get("ownedPaths")
            if not optional_string_list(migration_paths):
                validation.error(
                    f"Work package {package_id}.migrationPolicy.ownedPaths must be an array"
                )
                migration_paths = []
            for path in migration_paths:
                if not safe_relative_pattern(path):
                    validation.error(
                        f"Work package {package_id} has unsafe migration path: {path}"
                    )
                elif not any(
                    paths_overlap(path, allowed) for allowed in (allowed_paths or [])
                ):
                    validation.error(
                        f"Work package {package_id} migration path is outside allowedPaths: {path}"
                    )
            if migration.get("mode") == "owner" and not migration_paths:
                validation.error(
                    f"Work package {package_id} owning migrations requires ownedPaths"
                )
            for field in ("dataBackfill", "rollback"):
                if (
                    not isinstance(migration.get(field), str)
                    or not migration[field].strip()
                ):
                    validation.error(
                        f"Work package {package_id}.migrationPolicy.{field} is required"
                    )
        compatibility = package.get("compatibilityPolicy")
        if not isinstance(compatibility, dict):
            validation.error(
                f"Work package {package_id}.compatibilityPolicy must be an object"
            )
        else:
            for field in ("strategy", "supportedVersions", "removalGate"):
                if (
                    not isinstance(compatibility.get(field), str)
                    or not compatibility[field].strip()
                ):
                    validation.error(
                        f"Work package {package_id}.compatibilityPolicy.{field} is required"
                    )
        validate_acceptance_commands(
            package_id, package.get("acceptanceCommands"), validation
        )
        if not isinstance(package.get("verificationOnly"), bool):
            validation.error(
                f"Work package {package_id}.verificationOnly must be boolean"
            )
        declared_acceptance_commands = (
            {
                item.get("id"): item
                for item in package.get("acceptanceCommands", [])
                if isinstance(item, dict) and isinstance(item.get("id"), str)
            }
            if isinstance(package.get("acceptanceCommands"), list)
            else {}
        )
        scenarios = package.get("requiredAcceptanceScenarios")
        scenario_ids: set[str] = set()
        if not isinstance(scenarios, list) or not scenarios:
            validation.error(
                f"Work package {package_id}.requiredAcceptanceScenarios must be non-empty"
            )
        else:
            expected_scenario_fields = {
                "id",
                "assertion",
                "testSelector",
                "acceptanceCommandId",
            }
            linked_acceptance_commands: set[str] = set()
            for index, scenario in enumerate(scenarios):
                label = (
                    f"Work package {package_id}.requiredAcceptanceScenarios[{index}]"
                )
                if not isinstance(scenario, dict):
                    validation.error(f"{label} must be an object")
                    continue
                extra_fields = set(scenario) - expected_scenario_fields
                missing_fields = expected_scenario_fields - set(scenario)
                if extra_fields:
                    validation.error(
                        f"{label} has unsupported fields: {sorted(extra_fields)}"
                    )
                if missing_fields:
                    validation.error(
                        f"{label} is missing fields: {sorted(missing_fields)}"
                    )
                for field in sorted(expected_scenario_fields):
                    if (
                        not isinstance(scenario.get(field), str)
                        or not scenario[field].strip()
                    ):
                        validation.error(f"{label}.{field} is required")
                scenario_id = scenario.get("id")
                if isinstance(scenario_id, str) and scenario_id.strip():
                    if scenario_id in scenario_ids:
                        validation.error(
                            f"Work package {package_id} has duplicate acceptance scenario id: {scenario_id}"
                        )
                    scenario_ids.add(scenario_id)
                assertion = scenario.get("assertion")
                if isinstance(assertion, str) and len(assertion.strip()) < 24:
                    validation.error(
                        f"{label}.assertion is too short to be an observable acceptance claim"
                    )
                selector = scenario.get("testSelector")
                if isinstance(selector, str) and selector.strip():
                    if selector.startswith(("/", "~")) or ".." in selector.split("/"):
                        validation.error(
                            f"{label}.testSelector must be project-relative and traversal-free"
                        )
                acceptance_command_id = scenario.get("acceptanceCommandId")
                if (
                    isinstance(acceptance_command_id, str)
                    and acceptance_command_id.strip()
                ):
                    if acceptance_command_id in linked_acceptance_commands:
                        validation.error(
                            f"Work package {package_id} reuses acceptance command {acceptance_command_id} across scenarios"
                        )
                    linked_acceptance_commands.add(acceptance_command_id)
                    acceptance_command = declared_acceptance_commands.get(
                        acceptance_command_id
                    )
                    if acceptance_command is None:
                        validation.error(
                            f"{label}.acceptanceCommandId does not name a declared acceptance command"
                        )
                    else:
                        if isinstance(
                            selector, str
                        ) and not acceptance_command_executes_test_selector(
                            acceptance_command, selector
                        ):
                            validation.error(
                                f"{label}.acceptanceCommandId does not execute testSelector through a recognized test runner"
                            )
        output_artifacts = package.get("outputArtifacts")
        if output_artifacts is not None:
            if not isinstance(output_artifacts, list) or not output_artifacts:
                validation.error(
                    f"Work package {package_id}.outputArtifacts must be a non-empty array when present"
                )
            else:
                artifact_ids: set[str] = set()
                expected_artifact_fields = {
                    "id",
                    "kind",
                    "generatorPath",
                    "manifestPath",
                    "outputPaths",
                    "sourceContractRefs",
                    "generatorVersion",
                    "deterministic",
                    "verificationScenarioId",
                }
                for index, artifact in enumerate(output_artifacts):
                    label = f"Work package {package_id}.outputArtifacts[{index}]"
                    if not isinstance(artifact, dict):
                        validation.error(f"{label} must be an object")
                        continue
                    extra_fields = set(artifact) - expected_artifact_fields
                    missing_fields = expected_artifact_fields - set(artifact)
                    if extra_fields:
                        validation.error(
                            f"{label} has unsupported fields: {sorted(extra_fields)}"
                        )
                    if missing_fields:
                        validation.error(
                            f"{label} is missing fields: {sorted(missing_fields)}"
                        )
                    artifact_id = artifact.get("id")
                    if not isinstance(artifact_id, str) or not artifact_id.strip():
                        validation.error(f"{label}.id is required")
                    elif artifact_id in artifact_ids:
                        validation.error(
                            f"Work package {package_id} has duplicate output artifact id: {artifact_id}"
                        )
                    else:
                        artifact_ids.add(artifact_id)
                    if artifact.get("kind") != "generated_source_bundle":
                        validation.error(
                            f"{label}.kind must equal generated_source_bundle"
                        )
                    generator_path = artifact.get("generatorPath")
                    manifest_path = artifact.get("manifestPath")
                    for field, value in (
                        ("generatorPath", generator_path),
                        ("manifestPath", manifest_path),
                    ):
                        if not isinstance(value, str) or not safe_relative_file(value):
                            validation.error(
                                f"{label}.{field} must be one safe project-relative file"
                            )
                    output_paths = artifact.get("outputPaths")
                    if not non_empty_strings(output_paths):
                        validation.error(f"{label}.outputPaths must be non-empty")
                        output_paths = []
                    else:
                        for path in output_paths:
                            if not safe_relative_pattern(path):
                                validation.error(
                                    f"{label}.outputPaths contains unsafe path: {path}"
                                )
                    for field, path in (
                        ("generatorPath", generator_path),
                        ("manifestPath", manifest_path),
                        *(("outputPaths", path) for path in output_paths),
                    ):
                        if (
                            isinstance(path, str)
                            and safe_relative_pattern(path)
                            and not any(
                                path_is_covered_by_pattern(path, allowed)
                                for allowed in (allowed_paths or [])
                            )
                        ):
                            validation.error(
                                f"{label}.{field} is outside allowedPaths: {path}"
                            )
                    source_contract_refs = artifact.get("sourceContractRefs")
                    if not non_empty_strings(source_contract_refs):
                        validation.error(
                            f"{label}.sourceContractRefs must be non-empty"
                        )
                    else:
                        package_inputs = set(package.get("inputContracts", []))
                        for contract_ref in source_contract_refs:
                            if contract_ref not in package_inputs:
                                validation.error(
                                    f"{label}.sourceContractRefs references contract outside inputContracts: {contract_ref}"
                                )
                    generator_version = artifact.get("generatorVersion")
                    if not isinstance(generator_version, str) or not re.fullmatch(
                        r"v[1-9][0-9]*", generator_version
                    ):
                        validation.error(
                            f"{label}.generatorVersion must be a positive major version"
                        )
                    if artifact.get("deterministic") is not True:
                        validation.error(f"{label}.deterministic must be true")
                    verification_scenario_id = artifact.get("verificationScenarioId")
                    if (
                        not isinstance(verification_scenario_id, str)
                        or verification_scenario_id not in scenario_ids
                    ):
                        validation.error(
                            f"{label}.verificationScenarioId must name a required acceptance scenario"
                        )
        if "evidenceArtifacts" in package:
            validation.error(
                f"Work package {package_id}.evidenceArtifacts is forbidden; verifier evidence paths are derived internally"
            )
        if not non_empty_strings(package.get("stopConditions")):
            validation.error(
                f"Work package {package_id}.stopConditions must be non-empty"
            )
    cycle = dependency_cycle(packages)
    if cycle:
        validation.error(f"Work-package dependency cycle: {' -> '.join(cycle)}")
        return packages
    for index, (left_id, left_path) in enumerate(runnable):
        for right_id, right_path in runnable[index + 1 :]:
            ordered = package_reaches(left_id, right_id, packages) or package_reaches(
                right_id, left_id, packages
            )
            if (
                left_id != right_id
                and not ordered
                and paths_overlap(left_path, right_path)
            ):
                validation.error(
                    f"Concurrently runnable work-package paths overlap: "
                    f"{left_id}:{left_path} and {right_id}:{right_path}"
                )
    if not packages:
        validation.unresolved(
            "No implementation work packages are defined", "workPackages", "*"
        )
    return packages


def validate_execution_registry(
    root: Path,
    manifest: dict[str, Any],
    packages: dict[str, dict[str, Any]],
    validation: Validation,
    *,
    release_units: dict[str, dict[str, Any]] | None = None,
    reconciled_runs: frozenset[str] = frozenset(),
) -> None:
    registry_path = root / BUNDLE_DIR / "execution" / "work-package-runs.json"
    try:
        registry = read_object(registry_path)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        validation.error(f"Cannot read work-package execution registry: {exc}")
        return
    if registry.get("schemaVersion") != 1:
        validation.error("Execution registry schemaVersion must be 1")
    runs = registry.get("runs")
    if not isinstance(runs, list):
        validation.error("Execution registry runs must be an array")
        return
    seen_ids: set[str] = set()
    current_hash = manifest.get("freeze", {}).get("manifestHash")
    if release_units is None:
        release_units = {}
        relative = manifest.get("artifacts", {}).get("releaseUnits")
        if isinstance(relative, str):
            try:
                release_units = release_unit_index(
                    read_object(root / BUNDLE_DIR / relative)
                )
            except (OSError, ValueError, json.JSONDecodeError) as exc:
                validation.error(
                    f"Cannot read release units for execution registry: {exc}"
                )

    latest_completed_run_ids: dict[tuple[str, tuple[tuple[str, str], ...]], str] = {}
    latest_run_statuses: dict[tuple[str, tuple[tuple[str, str], ...]], str] = {}
    for candidate in runs:
        if not isinstance(candidate, dict):
            continue
        candidate_id = candidate.get("id")
        candidate_package = candidate.get("packageId")
        if not isinstance(candidate_id, str) or not isinstance(candidate_package, str):
            continue
        try:
            candidate_identity = tuple(
                sorted(run_execution_identity(candidate).items())
            )
        except ValueError:
            continue
        candidate_key = (candidate_package, candidate_identity)
        candidate_status = candidate.get("status")
        if isinstance(candidate_status, str):
            latest_run_statuses[candidate_key] = candidate_status
        if candidate_status == "completed":
            latest_completed_run_ids[candidate_key] = candidate_id

    def historical_freeze_exists(manifest_hash: str) -> bool:
        if not re.fullmatch(r"sha256:[0-9a-f]{64}", manifest_hash):
            return False
        snapshot = (
            root
            / BUNDLE_DIR
            / "execution"
            / "freeze-snapshots"
            / manifest_hash.removeprefix("sha256:")
        )
        try:
            snapshot_manifest = read_object(snapshot / "manifest.json")
            return (
                snapshot_manifest.get("freeze", {}).get("manifestHash") == manifest_hash
                and governance_hash(snapshot, snapshot_manifest) == manifest_hash
            )
        except (OSError, ValueError, json.JSONDecodeError):
            return False

    for index, run in enumerate(runs):
        if not isinstance(run, dict):
            validation.error(f"Execution run {index} must be an object")
            continue
        run_id = run.get("id")
        if not isinstance(run_id, str) or not run_id:
            validation.error(f"Execution run {index}.id is required")
        elif run_id in seen_ids:
            validation.error(f"Duplicate execution run ID: {run_id}")
        else:
            seen_ids.add(run_id)
        status = run.get("status")
        if status not in {"in_progress", "completed", "failed"}:
            validation.error(f"Execution run {run_id}.status is invalid")
        try:
            identity = run_execution_identity(run)
        except ValueError as exc:
            validation.error(f"Execution run {run_id} identity is invalid: {exc}")
            continue
        run_hash = identity.get("manifestHash")
        release_unit_id = identity.get("releaseUnitId")
        release_unit_hash = identity.get("releaseUnitHash")
        run_package: dict[str, Any] | None = None
        if release_unit_id is not None:
            unit = release_units.get(release_unit_id)
            if unit is None or unit.get("status") != "frozen":
                validation.error(
                    f"Execution run {run_id} references non-frozen release unit {release_unit_id}"
                )
            else:
                try:
                    snapshot = load_release_unit_snapshot(root, unit)
                    if snapshot.get("releaseUnitHash") != release_unit_hash:
                        raise ValueError(
                            f"Execution run {run_id} releaseUnitHash does not match its snapshot"
                        )
                    run_package = snapshot_package(snapshot, str(run.get("packageId")))
                except ValueError as exc:
                    validation.error(str(exc))
        else:
            if status == "in_progress" and run_hash != current_hash:
                validation.error(
                    f"Active execution run {run_id} is not bound to the current frozen hash"
                )
            elif status in {"completed", "failed"} and run_hash != current_hash:
                if not historical_freeze_exists(str(run_hash)):
                    validation.error(
                        f"Historical execution run {run_id} has no verifiable freeze snapshot"
                    )
            if run_hash == current_hash and run.get("packageId") in packages:
                run_package = packages[str(run.get("packageId"))]
        package_id = run.get("packageId")
        if (
            release_unit_id is None
            and run_hash == current_hash
            and package_id not in packages
        ):
            validation.error(
                f"Current execution run {run_id} references missing package {package_id}"
            )
        for field in ("assignedTo", "baselineCommit", "startedAt"):
            if not isinstance(run.get(field), str) or not run[field]:
                validation.error(f"Execution run {run_id}.{field} is required")
        baseline_tree_hash = run.get("baselineTreeHash")
        baseline_relative = run.get("baselineSnapshotPath")
        baseline_snapshot_hash = run.get("baselineSnapshotSha256")
        if not isinstance(baseline_tree_hash, str) or not baseline_tree_hash.startswith(
            "sha256:"
        ):
            validation.error(f"Execution run {run_id}.baselineTreeHash is invalid")
        if not isinstance(baseline_relative, str) or not safe_relative_file(
            baseline_relative
        ):
            validation.error(f"Execution run {run_id}.baselineSnapshotPath is invalid")
        else:
            baseline_path = root / BUNDLE_DIR / baseline_relative
            if not baseline_path.is_file():
                validation.error(f"Execution run {run_id} baseline snapshot is missing")
            elif (
                baseline_snapshot_hash
                != f"sha256:{hashlib.sha256(baseline_path.read_bytes()).hexdigest()}"
            ):
                validation.error(
                    f"Execution run {run_id} baseline snapshot hash mismatch"
                )
        if status in {"completed", "failed"}:
            relative = run.get("reportPath")
            expected_hash = run.get("reportSha256")
            if not isinstance(relative, str) or not safe_relative_file(relative):
                validation.error(f"Execution run {run_id}.reportPath is invalid")
                continue
            report_path = root / BUNDLE_DIR / relative
            if not report_path.is_file():
                validation.error(f"Execution run {run_id} report is missing")
                continue
            actual_hash = (
                f"sha256:{hashlib.sha256(report_path.read_bytes()).hexdigest()}"
            )
            if expected_hash != actual_hash:
                validation.error(f"Execution run {run_id} report hash mismatch")
                continue
            try:
                report = read_object(report_path)
            except (OSError, ValueError, json.JSONDecodeError) as exc:
                validation.error(f"Execution run {run_id} report cannot be read: {exc}")
                continue
            report_schema_version = report.get("schemaVersion")
            if report_schema_version not in SUPPORTED_REPORT_SCHEMA_VERSIONS:
                validation.error(
                    f"Execution run {run_id} report schemaVersion is unsupported"
                )
            report_identity = {key: report.get(key) for key in identity}
            if report.get("runId") != run_id or report_identity != identity:
                validation.error(f"Execution run {run_id} report identity mismatch")
            incompatible = (
                {"manifestHash"}
                if release_unit_id is not None
                else {"releaseUnitId", "releaseUnitHash"}
            )
            if any(field in report for field in incompatible):
                validation.error(
                    f"Execution run {run_id} report mixes authority identities"
                )
            if not isinstance(report.get("testedTreeHash"), str) or not report[
                "testedTreeHash"
            ].startswith("sha256:"):
                validation.error(f"Execution run {run_id} report lacks testedTreeHash")
            if not isinstance(report.get("ownedResultHashes"), dict):
                validation.error(
                    f"Execution run {run_id} report lacks ownedResultHashes"
                )
            if status == "completed" and report_schema_version == REPORT_SCHEMA_VERSION:
                if not isinstance(report.get("testedOwnedState"), dict):
                    validation.error(
                        f"Execution run {run_id} report lacks testedOwnedState"
                    )
                if not isinstance(report.get("testedOwnedStateSha256"), str):
                    validation.error(
                        f"Execution run {run_id} report lacks testedOwnedStateSha256"
                    )
            if not isinstance(report.get("sourceScopeFailures"), list):
                validation.error(
                    f"Execution run {run_id} report lacks sourceScopeFailures"
                )
            elif status == "completed" and report["sourceScopeFailures"]:
                validation.error(
                    f"Completed execution run {run_id} has source scope failures"
                )
            expected_pass = status == "completed"
            if report.get("passed") is not expected_pass:
                validation.error(
                    f"Execution run {run_id} status disagrees with its report"
                )
            if status == "completed" and run_package is not None:
                latest_key = (
                    str(package_id),
                    tuple(sorted(identity.items())),
                )
                check_current_outputs = (
                    latest_completed_run_ids.get(latest_key) == run_id
                    and run_id not in reconciled_runs
                    and latest_run_statuses.get(latest_key) != "in_progress"
                    and not (
                        release_unit_id is not None
                        and bool(
                            frozen_release_unit_superseders(
                                release_units, release_unit_id
                            )
                        )
                    )
                )
                for issue in validate_completed_run_integrity(
                    root,
                    run,
                    run_package,
                    str(run_hash) if release_unit_id is None else None,
                    check_current_outputs=check_current_outputs,
                    release_unit_id=release_unit_id,
                    release_unit_hash=release_unit_hash,
                ):
                    validation.error(
                        f"Execution run {run_id} integrity failure: {issue}"
                    )


def load_artifacts(
    bundle: Path, manifest: dict[str, Any], validation: Validation
) -> dict[str, dict[str, Any]]:
    artifacts = manifest.get("artifacts")
    if not isinstance(artifacts, dict):
        validation.error("manifest.artifacts must be an object")
        return {}
    loaded: dict[str, dict[str, Any]] = {}
    seen_paths: set[str] = set()
    for key, relative_path in artifacts.items():
        if not isinstance(relative_path, str) or not relative_path:
            validation.error(f"manifest.artifacts.{key} must be a path")
            continue
        if not safe_relative_file(relative_path):
            validation.error(f"manifest.artifacts.{key} is not a safe relative path")
            continue
        normalized_path = path_base(relative_path).as_posix()
        if normalized_path in seen_paths:
            validation.error(
                f"Artifact path is assigned more than once: {normalized_path}"
            )
            continue
        seen_paths.add(normalized_path)
        path = bundle / relative_path
        resolved_path = path.resolve()
        resolved_bundle = bundle.resolve()
        if (
            resolved_bundle not in (resolved_path, *resolved_path.parents)
            or path.is_symlink()
        ):
            validation.error(
                f"manifest.artifacts.{key} escapes the bundle or is a symlink"
            )
            continue
        try:
            loaded[key] = read_object(path)
        except (OSError, ValueError, json.JSONDecodeError) as exc:
            validation.error(f"Cannot read artifact {key} at {path}: {exc}")
    return loaded


def validate_catalog_bundle(
    root: Path, mode: str, *, reconciled_runs: frozenset[str] = frozenset()
) -> Validation:
    validation = Validation(mode)
    bundle = root / BUNDLE_DIR
    try:
        manifest = read_object(bundle / "manifest.json")
        policy = read_object(POLICY_PATH)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        validation.error(str(exc))
        return validation
    if manifest.get("schemaVersion") != 1:
        validation.error("manifest.schemaVersion must be 1")
    if manifest.get("status") not in {"draft", "review", "frozen"}:
        validation.error("manifest.status must be draft, review, or frozen")
    if mode == "review" and manifest.get("status") != "review":
        validation.error("Review validation requires manifest.status=review")
    tracks = manifest.get("tracks")
    if not non_empty_strings(tracks) or len(tracks) != len(set(tracks)):
        validation.error("manifest.tracks must contain unique track IDs")
        tracks = []
    artifacts = load_artifacts(bundle, manifest, validation)
    for field, policy_key in (
        ("requiredPageStates", "requiredPageStates"),
        ("dualTrackCapabilities", "defaultTrackCapabilities"),
        ("requiredNfrCategories", "requiredNfrCategories"),
    ):
        declared = manifest.get(field)
        if not optional_string_list(declared):
            validation.error(f"manifest.{field} must be an array")
        else:
            omitted = set(policy[policy_key]) - set(declared)
            if omitted:
                validation.error(
                    f"manifest.{field} cannot omit policy requirements: {', '.join(sorted(omitted))}"
                )
    required_artifacts = {
        "evidenceLedger",
        "conflictRegister",
        "decisionRegister",
        "glossary",
        "requirements",
        "journeys",
        "domains",
        "stateMachines",
        "contracts",
        "pageMatrix",
        "trackCoverage",
        "dataLifecycle",
        "aiScenarios",
        "entitlements",
        "nonFunctionalRequirements",
        "workPackages",
    }
    for key in sorted(required_artifacts - artifacts.keys()):
        validation.error(f"Required artifact is missing: {key}")
    for key, artifact in artifacts.items():
        if artifact.get("schemaVersion") != 1:
            validation.error(f"Artifact {key}.schemaVersion must be 1")
    catalog_release_units: dict[str, dict[str, Any]] = {}
    frozen_package_ids: set[str] = set()
    if "releaseUnits" in artifacts:
        try:
            catalog_release_units = release_unit_index(artifacts["releaseUnits"])
            for issue in validate_release_unit_snapshot_registry(
                root, catalog_release_units
            ):
                validation.error(issue)
            for unit_id, unit in catalog_release_units.items():
                if unit.get("status") != "frozen":
                    continue
                snapshot = load_release_unit_snapshot(root, unit)
                packages = (
                    snapshot.get("artifacts", {})
                    .get("workPackages", {})
                    .get("packages", [])
                )
                frozen_package_ids.update(
                    str(package["id"])
                    for package in packages
                    if isinstance(package, dict) and isinstance(package.get("id"), str)
                )
                for conflict in cross_unit_path_conflicts(
                    root,
                    catalog_release_units,
                    unit_id,
                    packages if isinstance(packages, list) else [],
                ):
                    validation.error(conflict)
        except ValueError as exc:
            validation.error(str(exc))
    if validation.errors:
        return validation

    evidence = validate_evidence(artifacts["evidenceLedger"], policy, root, validation)
    decisions = validate_decisions(artifacts["decisionRegister"], evidence, validation)
    validate_conflicts(artifacts["conflictRegister"], evidence, decisions, validation)
    requirements = validate_requirement_entries(
        object_list(artifacts["requirements"], "requirements", validation),
        "requirement",
        policy,
        evidence,
        decisions,
        validation,
    )
    if not requirements:
        validation.unresolved(
            "No product requirements are defined", "requirements", "*"
        )
    domains = validate_domains(artifacts["domains"], set(tracks), validation)
    validate_requirement_owners(requirements, "requirement", domains, validation)
    validate_glossary(artifacts["glossary"], domains, validation)
    contracts = validate_contracts(
        artifacts["contracts"], domains, requirements, evidence, validation
    )
    validate_state_machines(artifacts["stateMachines"], domains, contracts, validation)
    pages = validate_pages(
        artifacts["pageMatrix"], manifest, domains, decisions, validation
    )
    validate_journeys(artifacts["journeys"], pages, requirements, validation)
    validate_coverage(
        artifacts["trackCoverage"],
        manifest,
        domains,
        contracts,
        decisions,
        evidence,
        validation,
    )
    validate_data_lifecycle(
        artifacts["dataLifecycle"], domains, evidence, decisions, validation
    )
    validate_ai_scenarios(
        artifacts["aiScenarios"], domains, contracts, evidence, decisions, validation
    )
    validate_entitlements(
        artifacts["entitlements"],
        domains,
        requirements,
        decisions,
        contracts,
        validation,
    )
    nfr_entries = object_list(
        artifacts["nonFunctionalRequirements"], "requirements", validation
    )
    nfr = validate_requirement_entries(
        nfr_entries,
        "non-functional requirement",
        policy,
        evidence,
        decisions,
        validation,
    )
    validate_requirement_owners(nfr, "non-functional requirement", domains, validation)
    validate_nfr_verification(nfr, policy, evidence, decisions, validation)
    for category in manifest.get("requiredNfrCategories", []):
        matches = [entry for entry in nfr.values() if entry.get("category") == category]
        if len(matches) != 1:
            validation.error(
                f"NFR category {category} must have exactly one requirement"
            )
    packages = validate_work_packages(
        artifacts["workPackages"],
        contracts,
        requirements,
        decisions,
        validation,
        frozen_package_ids=frozen_package_ids,
    )
    validate_execution_registry(root, manifest, packages, validation, reconciled_runs=reconciled_runs)

    track_decision_refs = manifest.get("trackDecisionRefs")
    if not isinstance(track_decision_refs, dict):
        validation.error("manifest.trackDecisionRefs must be an object")
        track_decision_refs = {}
    for track in tracks:
        if not accepted_decision(track_decision_refs.get(track), decisions):
            validation.unresolved(
                f"Declared track {track} lacks an accepted scope decision",
                "tracks",
                track,
            )

    if mode == "freeze":
        if manifest.get("status") != "frozen":
            validation.error("Freeze validation requires manifest.status=frozen")
        freeze = manifest.get("freeze")
        if not isinstance(freeze, dict):
            validation.error("manifest.freeze must be an object")
        else:
            if not accepted_decision(freeze.get("decisionId"), decisions):
                validation.error("Frozen manifest requires an accepted freeze decision")
            for field in ("commit", "frozenAt", "manifestHash"):
                if not isinstance(freeze.get(field), str) or not freeze[field]:
                    validation.error(f"Frozen manifest requires freeze.{field}")
            if isinstance(freeze.get("manifestHash"), str):
                try:
                    actual_hash = governance_hash(bundle, manifest)
                    if freeze["manifestHash"] != actual_hash:
                        validation.error(
                            f"Governance hash mismatch: recorded {freeze['manifestHash']}, actual {actual_hash}"
                        )
                except (OSError, ValueError, json.JSONDecodeError) as exc:
                    validation.error(f"Cannot compute governance hash: {exc}")
    return validation


def _release_unit_readiness(
    root: Path,
    manifest: dict[str, Any],
    artifacts: dict[str, dict[str, Any]],
    units: dict[str, dict[str, Any]],
    unit: dict[str, Any],
    closure: dict[str, list[str]],
    base: Validation,
    validation: Validation,
) -> list[dict[str, str]]:
    deferred_items: list[dict[str, str]] = []
    for item in base.unresolved_items:
        kind = item["subjectKind"]
        identifier = item["subjectId"]
        in_scope = closure_contains(closure, kind, identifier)
        if kind == "conflicts":
            validation.error(
                f"Release unit {unit['id']} cannot classify an open blocking conflict as deferred: "
                f"{item['message']}"
            )
        elif in_scope:
            validation.unresolved(item["message"], kind, identifier)
        else:
            deferred_items.append(item)

    decisions = {
        item.get("id"): item
        for item in artifacts["decisionRegister"].get("decisions", [])
        if isinstance(item, dict) and isinstance(item.get("id"), str)
    }
    decision = decisions.get(unit.get("decisionRef"))
    if not isinstance(decision, dict) or decision.get("status") != "accepted":
        validation.error(
            f"Release unit {unit['id']} requires accepted decision {unit.get('decisionRef')}"
        )

    domain_index = {
        item.get("id"): item
        for item in artifacts["domains"].get("domains", [])
        if isinstance(item, dict)
    }
    for domain_id in closure["domains"]:
        domain = domain_index[domain_id]
        if not non_empty_strings(domain.get("invariants")):
            validation.error(
                f"Release-unit domain {domain_id} requires explicit invariants"
            )
        for field in ("commands", "queries", "emits", "consumes"):
            if not isinstance(domain.get(field), list):
                validation.error(
                    f"Release-unit domain {domain_id}.{field} must be an array"
                )

    page_index = {
        item.get("id"): item
        for item in artifacts["pageMatrix"].get("pages", [])
        if isinstance(item, dict)
    }
    for page_id in closure["pages"]:
        page = page_index[page_id]
        for field in ("entryConditions", "actions", "exitRoutes", "permissions"):
            if not isinstance(page.get(field), list):
                validation.error(
                    f"Release-unit page {page_id}.{field} must be an array"
                )

    machine_index = {
        item.get("id"): item
        for item in artifacts["stateMachines"].get("machines", [])
        if isinstance(item, dict)
    }
    for machine_id in closure["stateMachines"]:
        machine = machine_index[machine_id]
        states = (
            set(machine.get("states", []))
            if isinstance(machine.get("states"), list)
            else set()
        )
        terminals = (
            set(machine.get("terminalStates", []))
            if isinstance(machine.get("terminalStates"), list)
            else set()
        )
        outbound = {
            transition.get("from")
            for transition in machine.get("transitions", [])
            if isinstance(transition, dict)
        }
        missing = sorted(states - terminals - outbound)
        if missing:
            validation.error(
                f"Release-unit state machine {machine_id} has non-terminal states without transitions: {missing}"
            )

    package_index = {
        item.get("id"): item
        for item in artifacts["workPackages"].get("packages", [])
        if isinstance(item, dict)
    }
    candidate_packages = [
        package_index[identifier] for identifier in closure["workPackages"]
    ]
    for conflict in cross_unit_path_conflicts(
        root, units, str(unit["id"]), candidate_packages
    ):
        validation.error(conflict)

    for dependency_id in unit.get("dependsOnReleaseUnits", []):
        dependency = units.get(dependency_id)
        if not isinstance(dependency, dict) or dependency.get("status") != "frozen":
            validation.error(
                f"Release unit {unit['id']} dependency {dependency_id} is not frozen"
            )
        else:
            try:
                load_release_unit_snapshot(root, dependency)
            except ValueError as exc:
                validation.error(str(exc))
    return deferred_items


def validate_release_unit_bundle(
    root: Path, mode: str, release_unit_id: str
) -> Validation:
    validation = Validation(mode)
    bundle = root / BUNDLE_DIR
    try:
        manifest = read_object(bundle / "manifest.json")
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        validation.error(str(exc))
        return validation
    artifact_validation = Validation("draft")
    artifacts = load_artifacts(bundle, manifest, artifact_validation)
    validation.errors.extend(artifact_validation.errors)
    release_units_data = artifacts.get("releaseUnits")
    if not isinstance(release_units_data, dict):
        validation.error(
            "manifest.artifacts.releaseUnits is required for --release-unit validation"
        )
        return validation
    try:
        units = release_unit_index(release_units_data)
    except ValueError as exc:
        validation.error(str(exc))
        return validation
    for issue in validate_release_unit_snapshot_registry(root, units):
        validation.error(issue)
    unit = units.get(release_unit_id)
    if unit is None:
        validation.error(f"Unknown release unit: {release_unit_id}")
        return validation

    if mode == "freeze":
        if unit.get("status") != "frozen":
            validation.error(
                f"Release-unit freeze validation requires {release_unit_id}.status=frozen"
            )
            return validation
        try:
            snapshot = load_release_unit_snapshot(root, unit)
            decisions = (
                snapshot.get("artifacts", {})
                .get("decisionRegister", {})
                .get("decisions", [])
            )
            if not any(
                isinstance(item, dict)
                and item.get("id") == snapshot.get("decisionId")
                and item.get("status") == "accepted"
                for item in decisions
            ):
                validation.error(
                    f"Frozen release unit {release_unit_id} lacks its accepted decision"
                )
            packages = (
                snapshot.get("artifacts", {})
                .get("workPackages", {})
                .get("packages", [])
            )
            for conflict in cross_unit_path_conflicts(
                root,
                units,
                release_unit_id,
                packages if isinstance(packages, list) else [],
            ):
                validation.error(conflict)
        except ValueError as exc:
            validation.error(str(exc))
        validate_execution_registry(root, manifest, {}, validation, release_units=units)
        return validation

    expected_status = "review" if mode == "review" else "draft"
    if unit.get("status") != expected_status:
        validation.error(
            f"Release-unit {mode} validation requires {release_unit_id}.status={expected_status}"
        )
    reconciled_runs: set[str] = set()
    if mode == "review" and unit.get("baselineReconciliation") is not None:
        from baseline_reconciliation import reconciled_run_ids
        try:
            reconciled_runs = reconciled_run_ids(
                root, unit, units, artifacts["decisionRegister"].get("decisions", [])
            )
        except (KeyError, OSError, ValueError, json.JSONDecodeError) as exc:
            validation.error(str(exc))
            return validation
    base = validate_catalog_bundle(root, "draft", reconciled_runs=frozenset(reconciled_runs))
    validation.errors.extend(base.errors)
    if validation.errors:
        return validation
    try:
        closure = compute_release_unit_closure(manifest, artifacts, unit)
        deferred_items = _release_unit_readiness(
            root,
            manifest,
            artifacts,
            units,
            unit,
            closure,
            base,
            validation,
        )
        deferred = deferred_snapshot(bundle, manifest, deferred_items)
        validation.release_unit_context = {
            "manifest": manifest,
            "artifacts": artifacts,
            "units": units,
            "unit": unit,
            "closure": closure,
            "deferredSnapshot": deferred,
        }
        if mode == "draft":
            validation.warnings.extend(item["message"] for item in deferred_items)
    except (KeyError, TypeError, ValueError) as exc:
        validation.error(f"Release unit {release_unit_id} closure failed: {exc}")
    return validation


def validate_bundle(
    root: Path, mode: str, release_unit_id: str | None = None
) -> Validation:
    if release_unit_id is not None:
        return validate_release_unit_bundle(root, mode, release_unit_id)
    return validate_catalog_bundle(root, mode)


def main() -> int:
    args = parse_args()
    root = args.project_root.expanduser().resolve()
    validation = validate_bundle(root, args.mode, args.release_unit)
    payload = {
        "mode": args.mode,
        "valid": not validation.errors,
        "errorCount": len(validation.errors),
        "warningCount": len(validation.warnings),
        "errors": validation.errors,
        "warnings": validation.warnings,
    }
    if args.release_unit:
        payload["releaseUnitId"] = args.release_unit
        if validation.release_unit_context is not None:
            payload["closure"] = validation.release_unit_context["closure"]
            payload["deferredSnapshot"] = validation.release_unit_context[
                "deferredSnapshot"
            ]
    if args.report:
        write_object(args.report.expanduser().resolve(), payload)
    if args.as_json:
        print(json.dumps(payload, ensure_ascii=False, indent=2))
    else:
        print(
            f"Governance validation ({args.mode}): "
            f"{len(validation.errors)} error(s), {len(validation.warnings)} warning(s)"
        )
        for message in validation.errors:
            print(f"ERROR: {message}")
        for message in validation.warnings:
            print(f"WARN: {message}")
    return 0 if not validation.errors else 1


if __name__ == "__main__":
    raise SystemExit(main())
