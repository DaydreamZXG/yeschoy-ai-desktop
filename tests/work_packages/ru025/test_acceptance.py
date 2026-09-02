"""Read-only-source verification for the RU-024 functional client result."""

import json
import os
from pathlib import Path
import subprocess
from typing import Dict, List, Optional


ROOT = Path(__file__).resolve().parents[3]


def run_checked(
    command: List[str], *, timeout: int, env: Optional[Dict[str, str]] = None
) -> None:
    result = subprocess.run(
        command,
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
        timeout=timeout,
        env=env,
    )
    assert result.returncode == 0, result.stdout + result.stderr


def run_selected_vitest(selectors: List[str], tmp_path: Path) -> None:
    resolved = subprocess.run(
        [
            "node",
            "--input-type=module",
            "-e",
            "import {createRequire} from 'node:module'; const require=createRequire(import.meta.url); console.log(JSON.stringify({manifest:require.resolve('vitest/package.json'),react:import.meta.resolve('@vitejs/plugin-react')}))",
        ],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
        timeout=20,
    )
    modules = json.loads(resolved.stdout)
    manifest = Path(modules["manifest"])
    version = json.loads(manifest.read_text(encoding="utf-8"))["version"]
    assert f"vitest@{version}" in (ROOT / "pnpm-lock.yaml").read_text(encoding="utf-8")

    config = tmp_path / "vitest.config.mjs"
    config.write_text(
        f"import react from {json.dumps(modules['react'])};\n"
        "export default "
        + json.dumps(
            {
                "root": str(ROOT),
                "cacheDir": str(tmp_path / "vite-cache"),
                "resolve": {"alias": {"@": str(ROOT / "src")}},
                "test": {
                    "environment": "jsdom",
                    "globals": True,
                    "setupFiles": [
                        str(ROOT / "tests/setupGlobals.ts"),
                        str(ROOT / "tests/setupTests.ts"),
                    ],
                    "include": selectors,
                    "exclude": ["release/**", "work/**", "node_modules/**"],
                    "cache": {"dir": str(tmp_path / "vitest-cache")},
                },
            }
        ).removesuffix("}")
        + ", plugins: [react()]};\n",
        encoding="utf-8",
    )
    report_path = tmp_path / "vitest.json"
    result = subprocess.run(
        [
            "node",
            str(manifest.parent / "vitest.mjs"),
            "run",
            *selectors,
            "--config",
            str(config),
            "--reporter=json",
            "--outputFile",
            str(report_path),
        ],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
        timeout=240,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    report = json.loads(report_path.read_text(encoding="utf-8"))
    assert report["success"] is True
    assert report["numFailedTests"] == 0
    assert report["numPendingTests"] == 0
    assert report.get("numTodoTests", 0) == 0
    assert report["numTotalTests"] == report["numPassedTests"]
    assert report["numTotalTests"] > 0
    observed = {str(Path(item["name"]).resolve()) for item in report["testResults"]}
    expected = {str((ROOT / selector).resolve()) for selector in selectors}
    assert observed == expected


def test_native_session_price_and_activation(tmp_path: Path) -> None:
    account = (ROOT / "src-tauri/src/account_v2.rs").read_text(encoding="utf-8")
    activation = (ROOT / "src-tauri/src/tool_activation.rs").read_text(encoding="utf-8")

    assert "schema_version: 3" in account
    assert 'get("usd_exchange_rate")' in account
    assert "comparison_fx.map(decimal)" in account
    assert "stored.line_id != line_id" not in account
    assert "StatusCode::UNAUTHORIZED || response.status == StatusCode::FORBIDDEN" in account

    for required in (
        "configure_desktop_tool_v1",
        "/api/user/models",
        "/api/pricing",
        "/api/token/search",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_AUTH_TOKEN",
        'provider.insert("wire_api", value("responses"))',
        "atomic_write",
        "restore_snapshot",
        "delete_created_token",
        "model_supports_tool",
    ):
        assert required in activation

    projection = activation[
        activation.index("pub struct ToolActivationProjection") : activation.index(
            "impl ToolActivationProjection"
        )
    ]
    for forbidden in ("key", "token", "path", "origin"):
        assert forbidden not in projection.lower()

    environment = os.environ.copy()
    environment["RUSTUP_TOOLCHAIN"] = "stable"
    environment["CARGO_TARGET_DIR"] = str(tmp_path / "cargo-target")
    run_checked(
        ["cargo", "fmt", "--manifest-path", "src-tauri/Cargo.toml", "--check"],
        timeout=120,
        env=environment,
    )
    run_checked(
        [
            "cargo",
            "test",
            "--locked",
            "--manifest-path",
            "src-tauri/Cargo.toml",
            "--lib",
        ],
        timeout=900,
        env=environment,
    )


def test_shared_account_and_setup_ui(tmp_path: Path) -> None:
    app = (ROOT / "src/App.tsx").read_text(encoding="utf-8")
    session_hook = (ROOT / "src/account/useAccountSession.ts").read_text(encoding="utf-8")
    setup = (ROOT / "src/configuration/ConfigurationPreviewView.tsx").read_text(
        encoding="utf-8"
    )

    assert app.count("useAccountSession(") == 1
    assert "accountSession={accountSession}" in app
    assert "setProjection(null)" not in session_hook
    assert 'data-testid="configuration-apply-action"' in setup
    assert "activateDesktopTool" in setup
    assert "session.projection.models" in setup
    assert "configuration-apply-blocked" not in setup

    run_selected_vitest(
        [
            "src/account/session.test.ts",
            "src/configuration/activation.test.ts",
            "src/service-catalog/ServiceCatalogPanel.test.tsx",
            "src/workbench/Workbench.test.tsx",
            "src/workbench/Repair.test.tsx",
        ],
        tmp_path,
    )


def test_internal_candidate_build(tmp_path: Path) -> None:
    package = (ROOT / "package.json").read_text(encoding="utf-8")
    tauri = (ROOT / "src-tauri/tauri.conf.json").read_text(encoding="utf-8")
    candidate = (ROOT / "src-tauri/tauri.candidate.conf.json").read_text(
        encoding="utf-8"
    )
    renderer_activation = (ROOT / "src/configuration/activation.ts").read_text(
        encoding="utf-8"
    )

    assert '"version": "0.3.0"' in package
    assert '"version": "0.3.0"' in tauri
    assert '"createUpdaterArtifacts": false' in tauri
    assert '"hardenedRuntime": true' in candidate
    for forbidden in ("apiKey", "refreshToken", "configPath", "backupPath"):
        assert forbidden not in renderer_activation

    run_checked(["pnpm", "typecheck"], timeout=180)
    run_checked(
        [
            "pnpm",
            "exec",
            "vite",
            "build",
            "--outDir",
            str(tmp_path / "dist"),
            "--emptyOutDir",
        ],
        timeout=300,
    )
