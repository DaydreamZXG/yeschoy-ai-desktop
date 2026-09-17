"""Read-only current native baseline evidence; never opens an installed application."""

import os
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[3]


def test_current_codex_history_baseline(tmp_path):
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(tmp_path / "cargo-target")
    result = subprocess.run(
        [
            "cargo",
            "test",
            "--manifest-path",
            "src-tauri/Cargo.toml",
            "--lib",
            "codex_history_takeover::tests",
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
    (tmp_path / "cargo-test.log").write_text(output)
