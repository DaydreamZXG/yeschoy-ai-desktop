import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
NATIVE_ACCOUNT = ROOT / "src-tauri/src/account_v2.rs"
NATIVE_ACTIVATION = ROOT / "src-tauri/src/tool_activation.rs"
APP = ROOT / "src/App.tsx"
SESSION_HOOK = ROOT / "src/account/useAccountSession.ts"
SETUP = ROOT / "src/configuration/ConfigurationPreviewView.tsx"


def run(*command: str, timeout: int = 900) -> None:
    environment = os.environ.copy()
    environment["RUSTUP_TOOLCHAIN"] = "stable"
    subprocess.run(
        command,
        cwd=ROOT,
        check=True,
        timeout=timeout,
        env=environment,
    )


def test_native_session_price_and_activation():
    account = NATIVE_ACCOUNT.read_text(encoding="utf-8")
    activation = NATIVE_ACTIVATION.read_text(encoding="utf-8")

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

    run(
        "cargo",
        "fmt",
        "--manifest-path",
        "src-tauri/Cargo.toml",
        "--check",
    )
    run(
        "cargo",
        "test",
        "--locked",
        "--manifest-path",
        "src-tauri/Cargo.toml",
        "--lib",
    )


def test_shared_account_and_setup_ui():
    app = APP.read_text(encoding="utf-8")
    session_hook = SESSION_HOOK.read_text(encoding="utf-8")
    setup = SETUP.read_text(encoding="utf-8")

    assert app.count("useAccountSession(") == 1
    assert "accountSession={accountSession}" in app
    assert "setProjection(null)" not in session_hook
    assert 'data-testid="configuration-apply-action"' in setup
    assert "activateDesktopTool" in setup
    assert "session.projection.models" in setup
    assert "configuration-apply-blocked" not in setup

    run(
        "pnpm",
        "exec",
        "vitest",
        "run",
        "src/account/session.test.ts",
        "src/configuration/activation.test.ts",
        "src/service-catalog/ServiceCatalogPanel.test.tsx",
        "src/workbench/Workbench.test.tsx",
        "src/workbench/Repair.test.tsx",
        "--exclude",
        "release/**",
        "--exclude",
        "work/**",
        "--exclude",
        "tests/**",
    )


def test_internal_candidate_build():
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

    run("pnpm", "typecheck")
    run("pnpm", "build:renderer")
    run(
        "cargo",
        "check",
        "--locked",
        "--manifest-path",
        "src-tauri/Cargo.toml",
    )
