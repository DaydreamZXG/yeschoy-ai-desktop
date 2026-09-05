"""Behavioral evidence for RU-034. All generated artifacts stay outside source."""
from __future__ import annotations

import json
import os
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]


def run(*args: str, timeout: int = 1200, env: dict[str, str] | None = None) -> str:
    result = subprocess.run(args, cwd=ROOT, capture_output=True, text=True,
                            timeout=timeout, env=env)
    output = result.stdout + result.stderr
    assert result.returncode == 0, output[-24000:]
    return output


def modules() -> dict[str, str]:
    return json.loads(run("node", "--input-type=module", "-e",
        "import {createRequire} from 'node:module'; const r=createRequire(import.meta.url); "
        "console.log(JSON.stringify({vitest:r.resolve('vitest/package.json'),"
        "vite:r.resolve('vite/package.json'),react:import.meta.resolve('@vitejs/plugin-react')}))"))


def write_config(path: Path, plugin: str, configuration: dict) -> None:
    path.write_text(f"import react from {json.dumps(plugin)};\nexport default "
        + json.dumps(configuration).removesuffix("}")
        + ", plugins: [react()]};\n", encoding="utf-8")


def test_native_recovery_and_security(tmp_path: Path) -> None:
    environment = os.environ.copy()
    environment.setdefault("RUSTUP_TOOLCHAIN", "stable")
    # A caller may supply an existing cache; never build into the source tree.
    target = Path(environment.get("CARGO_TARGET_DIR", str(tmp_path / "cargo-target"))).resolve()
    assert target != ROOT and ROOT not in target.parents
    environment["CARGO_TARGET_DIR"] = str(target)
    args = ("--manifest-path", "src-tauri/Cargo.toml", "--locked", "--offline")
    output = run("cargo", "test", *args, "--lib", env=environment, timeout=1500)
    result = re.search(r"test result: ok\. (\d+) passed; 0 failed; 0 ignored;.*; 0 filtered out", output)
    assert result and int(result[1]) >= 331, output[-12000:]
    for name in (
        "restores_exact_original_bytes_for_all_seven_tools",
        "switch_keeps_first_baseline_and_rebases_later_unrelated_changes",
        "pending_switch_recovers_files_written_before_or_after_crash",
        "response_headers_empty_bodies_and_errors_are_not_verification",
        "stream_verification_requires_useful_complete_response_at_every_chunk_boundary",
        "cancelled_authorization_cannot_install_late_credentials",
        "upstream_redirects_never_forward_even_synthetic_credentials",
        "encrypted_baseline_authenticates_tool_key_and_bytes",
        "legacy_cleanup_never_removes_other_claude_mode_or_later_model_endpoint",
        "process_lock_is_exclusive_and_released_on_drop",
    ):
        assert re.search(rf"test [^\n]*::{name} \.\.\. ok", output), name
    # Include the executable target; no installer signing or live config writes.
    run("cargo", "check", *args, "--bin", "yeschoy-desktop", env=environment)
    run("cargo", "clippy", *args, "--all-targets", "--", "-D", "warnings", env=environment)
    run("cargo", "fmt", "--manifest-path", "src-tauri/Cargo.toml", "--", "--check", env=environment)


def test_renderer_login_and_reversible_workbench(tmp_path: Path) -> None:
    resolved = modules()
    config = tmp_path / "vitest.config.mjs"
    write_config(config, resolved["react"], {
        "root": str(ROOT), "cacheDir": str(tmp_path / "vite-cache"),
        "resolve": {"alias": {"@": str(ROOT / "src")}},
        "test": {"environment": "jsdom", "globals": True,
            "setupFiles": [str(ROOT / "tests/setupGlobals.ts"), str(ROOT / "tests/setupTests.ts")],
            "include": ["src/**/*.test.{ts,tsx}"],
            "cache": {"dir": str(tmp_path / "vitest-cache")}},
    })
    report = tmp_path / "vitest.json"
    run("node", str(Path(resolved["vitest"]).parent / "vitest.mjs"), "run",
        "--config", str(config), "--reporter=json", "--outputFile", str(report), timeout=600)
    result = json.loads(report.read_text(encoding="utf-8"))
    assert result["success"] and result["numTotalTests"] >= 325
    assert result["numTotalTests"] == result["numPassedTests"]
    assert result["numFailedTests"] == result["numPendingTests"] == 0
    names = {Path(item["name"]).name for item in result["testResults"]}
    assert {"useAccountSession.test.tsx", "connections.test.tsx", "Workbench.test.tsx",
        "ModelPicker.test.tsx", "BillingPrices.test.tsx", "billing.test.ts"} <= names


def test_client_build(tmp_path: Path) -> None:
    package = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))
    tauri = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
    candidate = json.loads((ROOT / "src-tauri/tauri.candidate.conf.json").read_text(encoding="utf-8"))
    assert package["version"] == tauri["version"] == "0.4.6"
    assert 'version = "0.4.6"' in (ROOT / "src-tauri/Cargo.toml").read_text(encoding="utf-8")
    assert candidate["bundle"]["createUpdaterArtifacts"] is False
    assert candidate["bundle"]["macOS"]["hardenedRuntime"] is True
    run("node", str(ROOT / "node_modules/typescript/bin/tsc"), "--noEmit", timeout=300)
    resolved = modules()
    config = tmp_path / "vite.config.mjs"
    out = tmp_path / "renderer"
    write_config(config, resolved["react"], {
        "root": str(ROOT / "src"), "base": "./", "cacheDir": str(tmp_path / "vite-cache"),
        "build": {"outDir": str(out), "emptyOutDir": True},
        "resolve": {"alias": {"@": str(ROOT / "src")}},
        "clearScreen": False, "envPrefix": ["VITE_", "TAURI_"],
    })
    run("node", str(Path(resolved["vite"]).parent / "bin/vite.js"), "build",
        "--config", str(config), "--configLoader", "runner", timeout=600)
    html = (out / "index.html").read_text(encoding="utf-8")
    assert "野菜API 桌面助手" in html
    assets = list((out / "assets").glob("*.js"))
    assert assets
    for asset in assets:
        content = asset.read_text(encoding="utf-8")
        assert "本地设计预览" not in content
        assert "Not available in visual fixture" not in content
