"""Fixture-only acceptance for the domestic mirror direct-origin route."""

import os
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[3]


def test_mirror_direct_origin_fallback(tmp_path):
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(tmp_path / "cargo-target")
    environment["CARGO_NET_OFFLINE"] = "true"
    environment["RUSTUP_TOOLCHAIN"] = "1.95.0-x86_64-apple-darwin"
    selectors = [
        "app_installation::origins::tests::mirror_catalog_uses_direct_route_only_after_public_transport_failure",
        "app_installation::tests::installer_public_mirror_failure_uses_direct_route_before_official",
        "app_installation::tests::installer_mirror_hash_failure_falls_back_to_official_without_credentials",
        "app_installation::tests::installer_resume_is_bound_to_artifact_not_merely_etag",
    ]
    for index, selector in enumerate(selectors):
        result = subprocess.run(
            [
                "cargo",
                "test",
                "--manifest-path",
                "src-tauri/Cargo.toml",
                "--lib",
                selector,
                "--",
                "--test-threads=1",
            ],
            cwd=ROOT,
            env=environment,
            text=True,
            capture_output=True,
            timeout=1140,
        )
        output = result.stdout + result.stderr
        assert result.returncode == 0, output
        assert "test result: ok." in output
        assert "0 failed" in output
        (tmp_path / f"cargo-test-{index}.log").write_text(output)
