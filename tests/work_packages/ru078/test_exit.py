"""Fixture-only exit evidence under verifier-owned read-only source isolation."""
import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[3]


def test_exit_fixtures(tmp_path):
    report = tmp_path / "renderer.json"
    renderer = subprocess.run(
        ["node", "tests/work_packages/ru078/run-renderer.mjs", str(report)],
        cwd=ROOT, text=True, capture_output=True, timeout=180,
    )
    assert renderer.returncode == 0, renderer.stdout + renderer.stderr
    result = json.loads(report.read_text())
    assert result["success"]
    assert result["numPassedTests"] >= 100
    assert result["numFailedTests"] == result["numPendingTests"] == 0
    assert all(assertion["status"] == "passed"
               for suite in result["testResults"] for assertion in suite["assertionResults"])
    (tmp_path / "renderer.log").write_text(renderer.stdout + renderer.stderr)

    # Select a built native lib-test executable; its source-identity test rejects
    # stale binaries. Never launch the real application, use real config or keys.
    deps = ROOT / "src-tauri/target/debug/deps"
    binaries = [p for p in deps.iterdir() if p.name.startswith("yeschoy_desktop_lib-")
                and "." not in p.name and p.is_file() and os.access(p, os.X_OK)]
    assert binaries, "Build native fixture tests first"
    binary = max(binaries, key=lambda p: p.stat().st_mtime_ns)
    for selector in ["exit_restore::tests", "connection_recovery::tests", "shutdown_coordinator::tests"]:
        native = subprocess.run([str(binary), selector, "--test-threads=1"],
                                cwd=ROOT, text=True, capture_output=True, timeout=120)
        output = native.stdout + native.stderr
        assert native.returncode == 0, output
        assert re.search(r"test result: ok\. [1-9][0-9]* passed; 0 failed; 0 ignored;", output), output
        if selector == "exit_restore::tests":
            assert "exit_restore_fixture_binary_matches_current_sources ... ok" in output
        (tmp_path / (selector.split("::")[0] + ".log")).write_text(output)
