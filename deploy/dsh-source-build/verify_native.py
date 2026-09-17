#!/usr/bin/env python3
from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

from policy import (
    MAX_ARTIFACT_BYTES,
    PolicyError,
    SAFE_NAME_RE,
    atomic_write_json,
    load_json,
    load_spec,
    sha256_file,
    validate_publisher,
)
from prepare_candidate import verify_source_receipt


TEAM_ID = re.compile(r"^TeamIdentifier=([A-Z0-9]{10})$", re.MULTILINE)
WINDOWS_THUMBPRINT = re.compile(r"^[0-9A-F]{40}$")


def run_tool(command: list[str]) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        command,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=180,
        env={**os.environ, "LC_ALL": "C"},
    )
    if result.returncode != 0:
        raise PolicyError("native_signature_verification_failed")
    return result


def verify_windows(artifact: Path, expected_publisher: str) -> None:
    if sys.platform != "win32":
        raise PolicyError("native_target_host_mismatch")
    if not WINDOWS_THUMBPRINT.fullmatch(expected_publisher):
        raise PolicyError("windows_publisher_policy_invalid")
    configured = os.environ.get("DSH_DESKTOP_WINDOWS_SIGNTOOL", "")
    signtool = Path(configured)
    if not configured or not signtool.is_absolute() or signtool.is_symlink() or not signtool.is_file():
        raise PolicyError("windows_signtool_unavailable")
    run_tool([str(signtool), "verify", "/pa", "/all", "/v", str(artifact)])

    powershell = shutil.which("powershell.exe") or shutil.which("pwsh.exe")
    if not powershell:
        raise PolicyError("powershell_unavailable")
    script = (
        "param([string]$ArtifactPath);"
        "$s=Get-AuthenticodeSignature -LiteralPath $ArtifactPath;"
        "[ordered]@{Status=[string]$s.Status;Thumbprint=[string]$s.SignerCertificate.Thumbprint}"
        "|ConvertTo-Json -Compress"
    )
    result = run_tool(
        [powershell, "-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script, str(artifact)]
    )
    try:
        observed = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise PolicyError("windows_signature_projection_invalid") from error
    if not isinstance(observed, dict) or set(observed) != {"Status", "Thumbprint"}:
        raise PolicyError("windows_signature_projection_invalid")
    if observed["Status"] != "Valid" or str(observed["Thumbprint"]).upper() != expected_publisher:
        raise PolicyError("windows_publisher_mismatch")


def verify_macos(artifact: Path, expected_publisher: str) -> None:
    if sys.platform != "darwin":
        raise PolicyError("native_target_host_mismatch")
    if not re.fullmatch(r"[A-Z0-9]{10}", expected_publisher):
        raise PolicyError("macos_publisher_policy_invalid")
    run_tool(["codesign", "--verify", "--deep", "--strict", "--verbose=2", str(artifact)])
    details = run_tool(["codesign", "-dv", "--verbose=4", str(artifact)])
    observed_text = f"{details.stdout}\n{details.stderr}"
    match = TEAM_ID.search(observed_text)
    if not match or match.group(1) != expected_publisher:
        raise PolicyError("macos_publisher_mismatch")
    run_tool(
        [
            "spctl",
            "--assess",
            "--type",
            "open",
            "--context",
            "context:primary-signature",
            "-vv",
            str(artifact),
        ]
    )
    run_tool(["xcrun", "stapler", "validate", str(artifact)])


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify a DSH Desktop artifact with native tools.")
    parser.add_argument("--artifact", required=True, type=Path)
    parser.add_argument("--target", required=True, choices=("win-x64", "mac-arm64", "mac-x64"))
    parser.add_argument("--source-receipt", required=True, type=Path)
    parser.add_argument("--spec", type=Path, default=Path(__file__).with_name("build-spec.json"))
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()
    try:
        spec, spec_raw = load_spec(arguments.spec)
        source_receipt, source_raw = load_json(arguments.source_receipt.absolute())
        source_sha256 = verify_source_receipt(source_receipt, source_raw, spec, spec_raw)
        target_spec = spec["targets"][arguments.target]
        publisher = validate_publisher(spec["signingPolicy"][arguments.target]["publisherId"])
        artifact = arguments.artifact.absolute()
        if not SAFE_NAME_RE.fullmatch(artifact.name):
            raise PolicyError("artifact_name_invalid")
        if artifact.suffix.lower() != target_spec["artifactExtension"]:
            raise PolicyError("artifact_extension_mismatch")
        artifact_sha256, artifact_size = sha256_file(artifact, MAX_ARTIFACT_BYTES)
        if arguments.target == "win-x64":
            verify_windows(artifact, publisher)
        else:
            verify_macos(artifact, publisher)
        receipt = {
            "schemaVersion": 1,
            "status": "verified",
            "target": arguments.target,
            "upstreamCommit": spec["upstreamCommit"],
            "appId": spec["appId"],
            "artifactFileName": artifact.name,
            "artifactSha256": artifact_sha256,
            "artifactSize": artifact_size,
            "sourceReceiptSha256": source_sha256,
            "signed": True,
            "notarized": target_spec["requiresNotarization"],
            "signatureKind": target_spec["signatureKind"],
            "publisherId": publisher,
            "verificationTool": target_spec["verificationTool"],
            "verifiedAt": dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat().replace(
                "+00:00", "Z"
            ),
        }
        atomic_write_json(arguments.output.absolute(), receipt)
        print("native_signature_verified")
        return 0
    except (OSError, subprocess.SubprocessError, PolicyError) as error:
        code = error.code if isinstance(error, PolicyError) else "native_verification_failed"
        print(code, file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
