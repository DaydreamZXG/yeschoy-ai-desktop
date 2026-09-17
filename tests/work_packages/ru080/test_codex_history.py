"""Fixture-only Codex history acceptance; never opens an installed profile."""

import os
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[3]


def test_codex_history_fixtures(tmp_path):
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(tmp_path / "cargo-target")
    environment["CARGO_NET_OFFLINE"] = "true"
    environment["RUSTUP_TOOLCHAIN"] = "1.95.0-x86_64-apple-darwin"
    selectors = [
        "codex_history_takeover::tests",
        "tool_activation::tests::codex_history_worker_does_not_block_async_runtime",
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
            timeout=840,
        )
        output = result.stdout + result.stderr
        assert result.returncode == 0, output
        assert "test result: ok." in output
        assert "0 failed" in output
        (tmp_path / f"cargo-test-{index}.log").write_text(output)
