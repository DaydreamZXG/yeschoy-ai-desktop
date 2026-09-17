from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
TOOLS = ROOT / "deploy" / "dsh-source-build"
VERIFY_SOURCE = TOOLS / "verify_source.py"
PREPARE_CANDIDATE = TOOLS / "prepare_candidate.py"
PRODUCTION_SPEC = TOOLS / "build-spec.json"
LICENSE = TOOLS / "LICENSE.deepseek-harness"


def run(command: list[str], *, cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    environment = {**os.environ, "YESCHOY_DSH_SOURCE_BUILD_TESTING": "1"}
    return subprocess.run(
        command,
        cwd=cwd or ROOT,
        env=environment,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=120,
    )


def git(source: Path, *arguments: str) -> str:
    result = run(["git", "-C", str(source), *arguments])
    assert result.returncode == 0, result.stderr
    return result.stdout.strip()


def write_json(path: Path, value: dict) -> None:
    path.write_text(json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8")


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify_command(source: Path, spec: Path, receipt: Path) -> list[str]:
    return [
        sys.executable,
        str(VERIFY_SOURCE),
        "--source",
        str(source),
        "--spec",
        str(spec),
        "--receipt",
        str(receipt),
        "--allow-fixture-spec",
    ]


def prepare_command(
    artifact: Path, source_receipt: Path, native_receipt: Path, spec: Path, output: Path
) -> list[str]:
    return [
        sys.executable,
        str(PREPARE_CANDIDATE),
        "--artifact",
        str(artifact),
        "--source-receipt",
        str(source_receipt),
        "--native-receipt",
        str(native_receipt),
        "--spec",
        str(spec),
        "--output",
        str(output),
        "--allow-fixture-spec",
    ]


def make_native_receipt(
    *, spec: dict, target: str, artifact: Path, source_receipt: Path, **changes: object
) -> dict:
    target_spec = spec["targets"][target]
    value = {
        "schemaVersion": 1,
        "status": "verified",
        "target": target,
        "upstreamCommit": spec["upstreamCommit"],
        "appId": spec["appId"],
        "artifactFileName": artifact.name,
        "artifactSha256": sha256(artifact),
        "artifactSize": artifact.stat().st_size,
        "sourceReceiptSha256": sha256(source_receipt),
        "signed": True,
        "notarized": target_spec["requiresNotarization"],
        "signatureKind": target_spec["signatureKind"],
        "publisherId": spec["signingPolicy"][target]["publisherId"],
        "verificationTool": target_spec["verificationTool"],
        "verifiedAt": "2026-09-16T09:00:00Z",
    }
    value.update(changes)
    return value


def test_source_build_boundary(tmp_path: Path) -> None:
    source = tmp_path / "source"
    (source / "apps" / "desktop").mkdir(parents=True)
    package = '{"name":"fixture","version":"0.1.6-alpha.1"}\n'
    (source / "package.json").write_text(package, encoding="utf-8")
    (source / "apps" / "desktop" / "package.json").write_text(package, encoding="utf-8")
    (source / "LICENSE").write_bytes(LICENSE.read_bytes())
    git(source, "init")
    git(source, "config", "user.email", "fixture@example.invalid")
    git(source, "config", "user.name", "Fixture")
    fixture_repository = "https://github.com/deepseek-ai/deepseek-harness-fixture.git"
    git(source, "remote", "add", "origin", fixture_repository)
    git(source, "add", "package.json", "apps/desktop/package.json", "LICENSE")
    git(source, "commit", "-m", "fixture")

    spec = json.loads(PRODUCTION_SPEC.read_text(encoding="utf-8"))
    spec.update(
        {
            "policyId": "ru082-fixture",
            "upstreamRepository": fixture_repository,
            "upstreamCommit": git(source, "rev-parse", "HEAD"),
            "upstreamTree": git(source, "rev-parse", "HEAD^{tree}"),
        }
    )
    spec["signingPolicy"] = {
        "win-x64": {"publisherId": "A" * 40},
        "mac-arm64": {"publisherId": "TEAMID1234"},
        "mac-x64": {"publisherId": "TEAMID1234"},
    }
    spec_path = tmp_path / "fixture-spec.json"
    write_json(spec_path, spec)

    source_receipt = tmp_path / "source-receipt.json"
    verified = run(verify_command(source, spec_path, source_receipt))
    assert verified.returncode == 0, verified.stderr
    source_value = json.loads(source_receipt.read_text(encoding="utf-8"))
    assert source_value["upstreamCommit"] == spec["upstreamCommit"]
    assert "source" not in source_value and "path" not in source_value

    (source / "package.json").write_text(package.replace("fixture", "dirty"), encoding="utf-8")
    dirty = run(verify_command(source, spec_path, tmp_path / "dirty.json"))
    assert dirty.returncode == 2
    assert dirty.stderr.strip() == "tracked_checkout_dirty"
    (source / "package.json").write_text(package, encoding="utf-8")

    wrong_commit_spec = {**spec, "upstreamCommit": "0" * 40}
    wrong_commit_path = tmp_path / "wrong-commit-spec.json"
    write_json(wrong_commit_path, wrong_commit_spec)
    wrong_commit = run(verify_command(source, wrong_commit_path, tmp_path / "wrong-commit.json"))
    assert wrong_commit.returncode == 2
    assert wrong_commit.stderr.strip() == "upstream_commit_mismatch"

    wrong_license_spec = json.loads(json.dumps(spec))
    wrong_license_spec["license"]["sha256"] = "0" * 64
    wrong_license_path = tmp_path / "wrong-license-spec.json"
    write_json(wrong_license_path, wrong_license_spec)
    wrong_license = run(verify_command(source, wrong_license_path, tmp_path / "wrong-license.json"))
    assert wrong_license.returncode == 2
    assert wrong_license.stderr.strip() == "license_digest_mismatch"

    artifact = tmp_path / "DeepSeek-Harness.exe"
    artifact.write_bytes(b"signed-fixture-artifact")
    native_receipt = tmp_path / "native-receipt.json"
    write_json(
        native_receipt,
        make_native_receipt(
            spec=spec, target="win-x64", artifact=artifact, source_receipt=source_receipt
        ),
    )
    candidate = tmp_path / "candidate.json"
    prepared = run(prepare_command(artifact, source_receipt, native_receipt, spec_path, candidate))
    assert prepared.returncode == 0, prepared.stderr
    candidate_value = json.loads(candidate.read_text(encoding="utf-8"))
    assert candidate_value["publishable"] is True
    assert candidate_value["label"] == "Yeschoy-built from official DeepSeek source"
    assert candidate_value["artifactSha256"] == sha256(artifact)
    assert not any("path" in key.casefold() for key in candidate_value)
    repeated = run(prepare_command(artifact, source_receipt, native_receipt, spec_path, candidate))
    assert repeated.returncode == 0, repeated.stderr

    def rejected(name: str, changes: dict, expected: str) -> None:
        receipt_path = tmp_path / f"{name}.json"
        write_json(
            receipt_path,
            make_native_receipt(
                spec=spec,
                target="win-x64",
                artifact=artifact,
                source_receipt=source_receipt,
                **changes,
            ),
        )
        result = run(
            prepare_command(
                artifact,
                source_receipt,
                receipt_path,
                spec_path,
                tmp_path / f"{name}-candidate.json",
            )
        )
        assert result.returncode == 2
        assert result.stderr.strip() == expected

    rejected("unsigned", {"signed": False}, "native_receipt_mismatch")
    rejected("wrong-publisher", {"publisherId": "B" * 40}, "native_receipt_mismatch")
    rejected("wrong-hash", {"artifactSha256": "0" * 64}, "native_receipt_mismatch")
    rejected("replay", {"sourceReceiptSha256": "0" * 64}, "native_receipt_mismatch")
    rejected("unknown-field", {"unexpected": True}, "invalid_native_receipt_fields")

    mac_artifact = tmp_path / "DeepSeek-Harness.dmg"
    mac_artifact.write_bytes(b"signed-and-notarized-fixture")
    mac_receipt = tmp_path / "mac-unnotarized.json"
    write_json(
        mac_receipt,
        make_native_receipt(
            spec=spec,
            target="mac-arm64",
            artifact=mac_artifact,
            source_receipt=source_receipt,
            notarized=False,
        ),
    )
    unnotarized = run(
        prepare_command(
            mac_artifact,
            source_receipt,
            mac_receipt,
            spec_path,
            tmp_path / "mac-candidate.json",
        )
    )
    assert unnotarized.returncode == 2
    assert unnotarized.stderr.strip() == "native_receipt_mismatch"

    unsigned_spec = json.loads(json.dumps(spec))
    unsigned_spec["signingPolicy"]["win-x64"]["publisherId"] = None
    unsigned_spec_path = tmp_path / "unsigned-spec.json"
    write_json(unsigned_spec_path, unsigned_spec)
    unsigned_source_receipt = tmp_path / "unsigned-source-receipt.json"
    unsigned_source = run(verify_command(source, unsigned_spec_path, unsigned_source_receipt))
    assert unsigned_source.returncode == 0, unsigned_source.stderr
    unsigned_native_receipt = tmp_path / "unsigned-policy-native.json"
    write_json(
        unsigned_native_receipt,
        make_native_receipt(
            spec=unsigned_spec,
            target="win-x64",
            artifact=artifact,
            source_receipt=unsigned_source_receipt,
        ),
    )
    unconfigured = run(
        prepare_command(
            artifact,
            unsigned_source_receipt,
            unsigned_native_receipt,
            unsigned_spec_path,
            tmp_path / "unsigned-policy-candidate.json",
        )
    )
    assert unconfigured.returncode == 2
    assert unconfigured.stderr.strip() == "signing_policy_unconfigured"
