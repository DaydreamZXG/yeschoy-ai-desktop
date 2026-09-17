"""Fixture-only WorkBuddy direct-configuration acceptance."""

import os
import json
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


def test_workbuddy_direct_support(tmp_path):
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(tmp_path / "cargo-target")
    environment["CARGO_NET_OFFLINE"] = "true"
    environment["RUSTUP_TOOLCHAIN"] = "1.94.0-x86_64-apple-darwin"

    selectors = [
        "tool_adapters::workbuddy::tests",
        "desktop_app_discovery_core::tests::workbuddy_windows_identity_is_exact_and_gui_only",
        "desktop_app_discovery::tests::windows_registered_and_path_fallbacks_cover_supported_installers",
        "open_connection::tests::all_six_open_targets_use_only_the_closed_native_request",
        "connection_recovery::tests::restores_exact_original_bytes_for_all_six_tools",
        "tool_activation::tests::protocol_support_is_target_specific",
    ]
    for index, selector in enumerate(selectors):
        output = run(
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
            environment,
        )
        assert "test result: ok." in output
        assert "0 failed" in output
        (tmp_path / f"cargo-{index}.log").write_text(output)

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
            "src/configuration/activation.test.ts",
            "src/configuration/preview.test.ts",
            "src/configuration/ConfigurationPreviewView.test.tsx",
            "src/workbench/appCatalog.test.ts",
        ],
        environment,
    )
    assert "failed" not in renderer.lower()
    assert "35 passed" in renderer
    (tmp_path / "renderer.log").write_text(renderer)
