#!/usr/bin/env python3
"""Initialize a non-overwriting, machine-readable product governance bundle."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import sys
from typing import Any


SKILL_ROOT = Path(__file__).resolve().parent.parent
POLICY_PATH = SKILL_ROOT / "assets" / "default-policy.json"
BUNDLE_DIR = ".product-governance"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project-root", required=True, type=Path)
    parser.add_argument("--product-name", required=True)
    parser.add_argument("--tracks", required=True, help="Comma-separated stable track IDs")
    return parser.parse_args()


def load_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        value = json.load(handle)
    if not isinstance(value, dict):
        raise ValueError(f"Expected a JSON object: {path}")
    return value


def dump_json(path: Path, value: object) -> None:
    path.write_text(
        json.dumps(value, ensure_ascii=False, indent=2, sort_keys=False) + "\n",
        encoding="utf-8",
    )


def normalized_tracks(raw: str) -> list[str]:
    tracks = [item.strip() for item in raw.split(",") if item.strip()]
    if not tracks:
        raise ValueError("At least one product track is required")
    if len(tracks) != len(set(tracks)):
        raise ValueError("Product tracks must be unique")
    for track in tracks:
        if not re.fullmatch(r"[a-z][a-z0-9_-]{1,39}", track):
            raise ValueError(f"Invalid track ID: {track}")
    return tracks


def build_files(product_name: str, tracks: list[str], policy: dict[str, Any]) -> dict[str, object]:
    created_at = datetime.now(timezone.utc).isoformat()
    artifacts = {
        "evidenceLedger": "evidence-ledger.json",
        "conflictRegister": "conflicts.json",
        "decisionRegister": "decisions.json",
        "glossary": "glossary.json",
        "requirements": "requirements.json",
        "journeys": "journeys.json",
        "domains": "domains.json",
        "stateMachines": "state-machines.json",
        "contracts": "contracts.json",
        "pageMatrix": "page-matrix.json",
        "trackCoverage": "track-coverage.json",
        "dataLifecycle": "data-lifecycle.json",
        "aiScenarios": "ai-scenarios.json",
        "entitlements": "entitlements.json",
        "nonFunctionalRequirements": "nfr.json",
        "workPackages": "work-packages.json",
        "releaseUnits": "release-units.json",
    }
    manifest = {
        "schemaVersion": 1,
        "product": {"name": product_name.strip(), "createdAt": created_at},
        "status": "draft",
        "authority": {
            "bundleRoot": BUNDLE_DIR,
            "precedence": [
                "user_confirmed",
                "applicable_policy",
                "frozen_decision",
                "code_observed",
                "existing_document",
                "inferred",
                "proposed",
            ],
        },
        "tracks": tracks,
        "trackDecisionRefs": {},
        "requiredPageStates": policy["requiredPageStates"],
        "dualTrackCapabilities": policy["defaultTrackCapabilities"],
        "requiredNfrCategories": policy["requiredNfrCategories"],
        "artifacts": artifacts,
        "freeze": {
            "decisionId": None,
            "commit": None,
            "frozenAt": None,
            "manifestHash": None,
        },
    }
    coverage = {
        "schemaVersion": 1,
        "capabilities": [
            {
                "capability": capability,
                "tracks": {
                    track: {
                        "status": "unknown",
                        "producer": None,
                        "consumers": [],
                        "contract": None,
                        "evidenceRefs": [],
                        "decisionRef": None,
                    }
                    for track in tracks
                },
            }
            for capability in policy["defaultTrackCapabilities"]
        ],
    }
    nfr = {
        "schemaVersion": 1,
        "requirements": [
            {
                "id": f"NFR-{category}",
                "category": category,
                "statement": f"Define and verify the {category} commercial requirement.",
                "scope": "target",
                "impact": "high",
                "state": "unknown",
                "owner": None,
                "evidenceRefs": [],
                "acceptance": [],
            }
            for category in policy["requiredNfrCategories"]
        ],
    }
    return {
        "manifest.json": manifest,
        "evidence-ledger.json": {"schemaVersion": 1, "entries": []},
        "conflicts.json": {"schemaVersion": 1, "conflicts": []},
        "decisions.json": {"schemaVersion": 1, "decisions": []},
        "glossary.json": {"schemaVersion": 1, "terms": []},
        "requirements.json": {"schemaVersion": 1, "requirements": []},
        "journeys.json": {"schemaVersion": 1, "actors": [], "journeys": []},
        "domains.json": {"schemaVersion": 1, "domains": []},
        "state-machines.json": {"schemaVersion": 1, "machines": []},
        "contracts.json": {"schemaVersion": 1, "contracts": []},
        "page-matrix.json": {"schemaVersion": 1, "stateProfiles": [], "pages": []},
        "track-coverage.json": coverage,
        "data-lifecycle.json": {"schemaVersion": 1, "resources": []},
        "ai-scenarios.json": {"schemaVersion": 1, "scenarios": []},
        "entitlements.json": {"schemaVersion": 1, "capabilities": []},
        "nfr.json": nfr,
        "work-packages.json": {"schemaVersion": 1, "packages": []},
        "release-units.json": {"schemaVersion": 1, "releaseUnits": []},
    }


def main() -> int:
    args = parse_args()
    root = args.project_root.expanduser().resolve()
    if not root.is_dir():
        print(f"Project root does not exist: {root}", file=sys.stderr)
        return 2
    try:
        tracks = normalized_tracks(args.tracks)
        policy = load_json(POLICY_PATH)
        files = build_files(args.product_name, tracks, policy)
    except (KeyError, ValueError, json.JSONDecodeError) as exc:
        print(str(exc), file=sys.stderr)
        return 2

    bundle = root / BUNDLE_DIR
    collisions = [bundle / name for name in files if (bundle / name).exists()]
    if collisions:
        print("Refusing to overwrite existing governance files:", file=sys.stderr)
        for path in collisions:
            print(f"- {path}", file=sys.stderr)
        return 3

    bundle.mkdir(parents=True, exist_ok=True)
    for name, value in files.items():
        dump_json(bundle / name, value)
    (bundle / "reports").mkdir(exist_ok=True)
    execution = bundle / "execution"
    execution.mkdir(exist_ok=True)
    dump_json(execution / "work-package-runs.json", {"schemaVersion": 1, "runs": []})
    (execution / "reports").mkdir(exist_ok=True)
    print(f"Initialized {len(files)} governance files in {bundle}")
    print("Status remains draft; high-impact unknowns intentionally block freeze.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
