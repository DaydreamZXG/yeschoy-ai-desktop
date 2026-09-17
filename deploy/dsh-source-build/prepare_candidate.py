#!/usr/bin/env python3
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path
from typing import Any

from policy import (
    MAX_ARTIFACT_BYTES,
    PolicyError,
    SAFE_NAME_RE,
    SHA256_RE,
    atomic_write_json,
    load_json,
    load_spec,
    require_exact_keys,
    sha256_bytes,
    sha256_file,
    validate_publisher,
)


SOURCE_RECEIPT_FIELDS = {
    "schemaVersion",
    "status",
    "policyId",
    "specSha256",
    "upstreamRepository",
    "upstreamCommit",
    "upstreamTree",
    "rootVersion",
    "desktopVersion",
    "appId",
    "licenseSpdx",
    "licenseSha256",
}
NATIVE_RECEIPT_FIELDS = {
    "schemaVersion",
    "status",
    "target",
    "upstreamCommit",
    "appId",
    "artifactFileName",
    "artifactSha256",
    "artifactSize",
    "sourceReceiptSha256",
    "signed",
    "notarized",
    "signatureKind",
    "publisherId",
    "verificationTool",
    "verifiedAt",
}
UTC_TIMESTAMP = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,6})?Z$")


def verify_source_receipt(receipt: dict[str, Any], raw: bytes, spec: dict, spec_raw: bytes) -> str:
    require_exact_keys(receipt, SOURCE_RECEIPT_FIELDS, "invalid_source_receipt_fields")
    expected = {
        "schemaVersion": 1,
        "status": "verified",
        "policyId": spec["policyId"],
        "specSha256": sha256_bytes(spec_raw),
        "upstreamRepository": spec["upstreamRepository"],
        "upstreamCommit": spec["upstreamCommit"],
        "upstreamTree": spec["upstreamTree"],
        "rootVersion": spec["rootVersion"],
        "desktopVersion": spec["desktopVersion"],
        "appId": spec["appId"],
        "licenseSpdx": spec["license"]["spdx"],
        "licenseSha256": spec["license"]["sha256"],
    }
    if receipt != expected:
        raise PolicyError("source_receipt_mismatch")
    return sha256_bytes(raw)


def verify_native_receipt(
    receipt: dict[str, Any],
    raw: bytes,
    spec: dict,
    target: str,
    source_receipt_sha256: str,
    artifact_name: str,
    artifact_sha256: str,
    artifact_size: int,
) -> tuple[str, str]:
    require_exact_keys(receipt, NATIVE_RECEIPT_FIELDS, "invalid_native_receipt_fields")
    target_spec = spec["targets"][target]
    publisher = validate_publisher(spec["signingPolicy"][target]["publisherId"])
    if not isinstance(receipt.get("verifiedAt"), str) or not UTC_TIMESTAMP.fullmatch(
        receipt["verifiedAt"]
    ):
        raise PolicyError("native_receipt_timestamp_invalid")
    expected = {
        "schemaVersion": 1,
        "status": "verified",
        "target": target,
        "upstreamCommit": spec["upstreamCommit"],
        "appId": spec["appId"],
        "artifactFileName": artifact_name,
        "artifactSha256": artifact_sha256,
        "artifactSize": artifact_size,
        "sourceReceiptSha256": source_receipt_sha256,
        "signed": True,
        "notarized": target_spec["requiresNotarization"],
        "signatureKind": target_spec["signatureKind"],
        "publisherId": publisher,
        "verificationTool": target_spec["verificationTool"],
        "verifiedAt": receipt["verifiedAt"],
    }
    if receipt != expected:
        raise PolicyError("native_receipt_mismatch")
    return sha256_bytes(raw), publisher


def prepare_candidate(
    artifact: Path,
    source_receipt_path: Path,
    native_receipt_path: Path,
    spec: dict,
    spec_raw: bytes,
) -> dict[str, Any]:
    source_receipt, source_raw = load_json(source_receipt_path)
    source_sha256 = verify_source_receipt(source_receipt, source_raw, spec, spec_raw)
    native_receipt, native_raw = load_json(native_receipt_path)
    target = native_receipt.get("target")
    if target not in spec["targets"]:
        raise PolicyError("unsupported_target")

    artifact_name = artifact.name
    if not SAFE_NAME_RE.fullmatch(artifact_name) or artifact_name != str(Path(artifact_name)):
        raise PolicyError("artifact_name_invalid")
    target_spec = spec["targets"][target]
    if Path(artifact_name).suffix.lower() != target_spec["artifactExtension"]:
        raise PolicyError("artifact_extension_mismatch")
    artifact_sha256, artifact_size = sha256_file(artifact, MAX_ARTIFACT_BYTES)
    native_sha256, publisher = verify_native_receipt(
        native_receipt,
        native_raw,
        spec,
        target,
        source_sha256,
        artifact_name,
        artifact_sha256,
        artifact_size,
    )
    if not SHA256_RE.fullmatch(native_sha256):
        raise PolicyError("native_receipt_digest_invalid")

    return {
        "schemaVersion": 1,
        "status": "prepared",
        "publishable": True,
        "label": "Yeschoy-built from official DeepSeek source",
        "target": target,
        "upstreamRepository": spec["upstreamRepository"],
        "upstreamCommit": spec["upstreamCommit"],
        "upstreamTree": spec["upstreamTree"],
        "version": spec["desktopVersion"],
        "appId": spec["appId"],
        "artifactFileName": artifact_name,
        "artifactSha256": artifact_sha256,
        "artifactSize": artifact_size,
        "sourceReceiptSha256": source_sha256,
        "nativeReceiptSha256": native_sha256,
        "licenseSpdx": spec["license"]["spdx"],
        "licenseSha256": spec["license"]["sha256"],
        "signatureKind": target_spec["signatureKind"],
        "publisherId": publisher,
        "verifiedAt": native_receipt["verifiedAt"],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Prepare one signed DSH Desktop candidate receipt.")
    parser.add_argument("--artifact", required=True, type=Path)
    parser.add_argument("--source-receipt", required=True, type=Path)
    parser.add_argument("--native-receipt", required=True, type=Path)
    parser.add_argument("--spec", type=Path, default=Path(__file__).with_name("build-spec.json"))
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--allow-fixture-spec", action="store_true", help=argparse.SUPPRESS)
    arguments = parser.parse_args()
    try:
        spec, spec_raw = load_spec(arguments.spec, allow_fixture=arguments.allow_fixture_spec)
        candidate = prepare_candidate(
            arguments.artifact.absolute(),
            arguments.source_receipt.absolute(),
            arguments.native_receipt.absolute(),
            spec,
            spec_raw,
        )
        atomic_write_json(arguments.output.absolute(), candidate, idempotent=True)
        print("candidate_prepared")
        return 0
    except (OSError, PolicyError) as error:
        code = error.code if isinstance(error, PolicyError) else "candidate_preparation_failed"
        print(code, file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
