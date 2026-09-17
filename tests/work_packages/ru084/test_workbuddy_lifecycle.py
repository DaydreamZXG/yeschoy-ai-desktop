"""Fixture-only WorkBuddy lifecycle acceptance."""

import json
import os
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[3]


def run(command, environment, timeout=900):
    result = subprocess.run(
        command,
        cwd=ROOT,
        env=environment,
        text=True,
        capture_output=True,
        timeout=timeout,
    )
    output = result.stdout + result.stderr
    assert result.returncode == 0, output
    return output


def test_workbuddy_lifecycle_support(tmp_path):
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(tmp_path / "cargo-target")
    environment["CARGO_NET_OFFLINE"] = "true"
    environment["RUSTUP_TOOLCHAIN"] = "1.94.0-x86_64-apple-darwin"

    native = run(
        [
            "cargo",
            "test",
            "--manifest-path",
            "src-tauri/Cargo.toml",
            "--lib",
            "exit_restore::tests::exit_restore_partial_failure_does_not_skip_other_tools",
            "--",
            "--test-threads=1",
        ],
        environment,
    )
    assert "test result: ok." in native
    assert "0 failed" in native

    source = (ROOT / "src-tauri/src/account_v2.rs").read_text()
    logout = source.split("pub async fn account_logout_v2", 1)[1].split(
        "#[tauri::command]", 1
    )[0]
    assert '"workbuddy"' in logout
    assert "request_diagnostics::clear(tool)" in logout

    vitest_config = tmp_path / "vitest.config.mjs"
    vitest_config.write_text(
        "export default "
        + json.dumps(
            {
                "resolve": {"alias": {"@": str(ROOT / "src")}},
                "test": {
                    "cache": False,
                    "environment": "jsdom",
                    "setupFiles": [
                        str(ROOT / "tests/setupGlobals.ts"),
                        str(ROOT / "tests/setupTests.ts"),
                    ],
                    "globals": True,
                },
            }
        )
    )
    renderer = run(
        [
            "pnpm",
            "exec",
            "vitest",
            "run",
            "--config",
            str(vitest_config),
            "--dir",
            "src",
            "src/settings/QuitAssistant.test.tsx",
        ],
        environment,
    )
    assert "failed" not in renderer.lower()
    assert "passed" in renderer
