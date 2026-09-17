"""Closed-set WorkBuddy regression parity acceptance."""

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


def test_workbuddy_regression_parity(tmp_path):
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
            "tool_activation::tests::inspection_regression_one_adapter_panic_keeps_five_other_results",
            "--",
            "--test-threads=1",
        ],
        environment,
    )
    assert "test result: ok." in native
    assert "0 failed" in native

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
            "src/configuration/connections.test.tsx",
            "src/configuration/DailyUse.test.tsx",
            "src/workbench/AppGlyph.test.tsx",
            "src/workbench/Workbench.test.tsx",
        ],
        environment,
    )
    assert "failed" not in renderer.lower()
    assert "passed" in renderer
