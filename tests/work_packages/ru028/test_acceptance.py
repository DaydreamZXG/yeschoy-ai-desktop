from __future__ import annotations

import json
import os
import platform
import struct
import subprocess
from pathlib import Path
from typing import Dict, List, Mapping, Optional


ROOT = Path(__file__).resolve().parents[3]


def run(
    *args: str,
    cwd: Path = ROOT,
    timeout: int = 1_200,
    env: Optional[Mapping[str, str]] = None,
) -> str:
    completed = subprocess.run(
        args,
        cwd=cwd,
        check=True,
        capture_output=True,
        text=True,
        timeout=timeout,
        env=env,
    )
    return completed.stdout + completed.stderr


def read(relative: str) -> str:
    return (ROOT / relative).read_text(encoding="utf-8")


def rust_commands() -> tuple[str, str, str]:
    host = "x86_64-apple-darwin" if platform.machine() == "x86_64" else "aarch64-apple-darwin"
    root = Path.home() / ".rustup" / "toolchains" / f"1.94.0-{host}" / "bin"
    if (root / "cargo").is_file():
        return str(root / "cargo"), str(root / "rustc"), str(root / "rustdoc")
    return "cargo", "rustc", "rustdoc"


def test_native_capability_and_transaction_regressions(tmp_path: Path) -> None:
    cargo, rustc, rustdoc = rust_commands()
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(tmp_path / "cargo-target")
    environment["RUSTC"] = rustc
    environment["RUSTDOC"] = rustdoc
    output = run(
        cargo,
        "test",
        "--locked",
        "--manifest-path",
        "src-tauri/Cargo.toml",
        "--lib",
        env=environment,
        timeout=1_500,
    )
    assert "test result: ok. 62 passed" in output

    lines = read("src-tauri/src/connectivity_core.rs")
    assert 'root_url: "https://yeschoy.com"' in lines
    assert 'root_url: "https://yeschoy.pro"' in lines
    assert 'root_url: "https://api.yeschoy.com"' not in lines

    registry = read("src-tauri/src/tool_adapters/mod.rs")
    activation = read("src-tauri/src/tool_activation.rs")
    account = read("src-tauri/src/account_v2.rs")
    for tool_id in (
        "claude_code",
        "claude_desktop",
        "codex_desktop",
        "pi",
        "dsh_web",
        "hermes",
        "openclaw",
    ):
        assert f'"{tool_id}"' in registry
        assert f'"{tool_id}"' in activation
    assert "version: version.into()" in registry
    assert "can_attempt(tool_id, &observed)" in registry
    assert "tool_request_verified" in activation
    assert activation.index("verify_adapter(") < activation.rindex("tool_request_verified")
    assert "only refresh_access may prove the account session itself expired" in account


def resolve_node_modules() -> Dict[str, str]:
    output = run(
        "node",
        "--input-type=module",
        "-e",
        "import {createRequire} from 'node:module'; const require=createRequire(import.meta.url); console.log(JSON.stringify({vitest:require.resolve('vitest/package.json'),vite:require.resolve('vite/package.json'),react:import.meta.resolve('@vitejs/plugin-react')}))",
    )
    return json.loads(output)


def run_selected_vitest(selectors: List[str], tmp_path: Path) -> None:
    modules = resolve_node_modules()
    manifest = Path(modules["vitest"])
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
    report = tmp_path / "vitest.json"
    run(
        "node",
        str(manifest.parent / "vitest.mjs"),
        "run",
        *selectors,
        "--config",
        str(config),
        "--reporter=json",
        "--outputFile",
        str(report),
        timeout=600,
    )
    result = json.loads(report.read_text(encoding="utf-8"))
    assert result["success"] is True
    assert result["numTotalTests"] == result["numPassedTests"]
    assert result["numFailedTests"] == 0
    assert result["numPendingTests"] == 0


def test_renderer_compatibility_states(tmp_path: Path) -> None:
    run_selected_vitest(
        [
            "src/account/session.test.ts",
            "src/configuration/activation.test.ts",
            "src/configuration/billing.test.ts",
            "src/configuration/preview.test.ts",
            "src/service-catalog/access-plan.test.ts",
            "src/workbench/Workbench.test.tsx",
        ],
        tmp_path,
    )
    view = read("src/configuration/ConfigurationPreviewView.tsx")
    for tool_id in (
        "claude_code",
        "claude_desktop",
        "codex_desktop",
        "pi",
        "dsh_web",
        "hermes",
        "openclaw",
    ):
        assert f'id: "{tool_id}"' in view
    assert "selectedInstallationId" in view
    assert "selectedBillingGroup" in view
    assert "supportedEndpointTypes" in view
    assert "完成真实回复" in view


def test_seven_target_secret_and_failure_boundaries() -> None:
    credentials = read("src-tauri/src/tool_credentials.rs")
    activation = read("src-tauri/src/tool_activation.rs")
    renderer = read("src/configuration/activation.ts")
    main = read("src-tauri/src/main.rs")

    assert 'const SERVICE: &str = "com.yeschoy.desktop.tool-credential.v2"' in credentials
    assert "keyring::v1" in credentials
    assert "credential-helper-openclaw" in credentials
    assert "take(16 * 1024 + 1)" in credentials
    assert "credential_helper_exit_code" in main
    assert "api_key" not in renderer.lower()
    assert "access_token" not in renderer.lower()
    assert "secret" not in renderer.lower()
    assert "deny_unknown_fields" in activation
    assert "tool_credentials::restore" in activation
    assert "delete_created_token" in activation
    assert "restore_after_failure" in activation

    adapters = {
        "claude_code": "src-tauri/src/tool_adapters/claude_code.rs",
        "claude_desktop": "src-tauri/src/tool_adapters/claude_desktop.rs",
        "codex_desktop": "src-tauri/src/tool_adapters/codex_desktop.rs",
        "pi": "src-tauri/src/tool_adapters/pi.rs",
        "dsh_web": "src-tauri/src/tool_adapters/dsh_web.rs",
        "hermes": "src-tauri/src/tool_adapters/hermes.rs",
        "openclaw": "src-tauri/src/tool_adapters/openclaw.rs",
    }
    for tool_id, relative in adapters.items():
        source = read(relative)
        assert "FileTransaction" in source, tool_id
        assert "VerificationFailed" in source, tool_id
        assert "rollback" in source, tool_id
    assert "key_cmd" in read(adapters["hermes"])
    assert '"source":"exec"' in read(adapters["openclaw"])
    assert ".env(\"YESCHOY_DSH_API_KEY\", key)" in read(adapters["dsh_web"])
    assert 'format!("!{helper}")' in read(adapters["pi"])


def png_size(relative: str) -> tuple[int, int]:
    payload = (ROOT / relative).read_bytes()
    assert payload[:8] == b"\x89PNG\r\n\x1a\n"
    return struct.unpack(">II", payload[16:24])


def test_release_candidate_inputs_and_update_gate(tmp_path: Path) -> None:
    modules = resolve_node_modules()
    vite_manifest = Path(modules["vite"])
    config = tmp_path / "vite.config.mjs"
    config.write_text(
        f"import react from {json.dumps(modules['react'])};\n"
        "export default "
        + json.dumps(
            {
                "root": str(ROOT / "src"),
                "base": "./",
                "cacheDir": str(tmp_path / "vite-cache"),
                "build": {
                    "outDir": str(tmp_path / "dist"),
                    "emptyOutDir": True,
                },
                "resolve": {"alias": {"@": str(ROOT / "src")}},
                "clearScreen": False,
                "envPrefix": ["VITE_", "TAURI_"],
            }
        ).removesuffix("}")
        + ", plugins: [react()]};\n",
        encoding="utf-8",
    )
    run("node", str(ROOT / "node_modules/typescript/bin/tsc"), "--noEmit", timeout=300)
    run(
        "node",
        str(vite_manifest.parent / "bin/vite.js"),
        "build",
        "--config",
        str(config),
        "--configLoader",
        "runner",
        timeout=600,
    )

    package = json.loads(read("package.json"))
    tauri = json.loads(read("src-tauri/tauri.conf.json"))
    candidate = json.loads(read("src-tauri/tauri.candidate.conf.json"))
    cargo = read("src-tauri/Cargo.toml")
    readiness = read("src/candidate/readiness.ts")
    assert package["version"] == "0.4.1"
    assert tauri["version"] == "0.4.1"
    assert 'version = "0.4.1"' in cargo
    assert 'CANDIDATE_VERSION = "0.4.1"' in readiness
    assert tauri["productName"] == "野菜API"
    assert tauri["identifier"] == "com.yeschoy.desktop"
    assert candidate["bundle"]["active"] is True
    assert candidate["bundle"]["createUpdaterArtifacts"] is False
    assert tauri["bundle"]["createUpdaterArtifacts"] is False
    assert "automaticUpdateEnabled: false" in readiness
    assert "tauri-plugin-updater" not in cargo
    assert "tauri_plugin_updater" not in read("src-tauri/src/lib.rs")

    assert png_size("src-tauri/icons/32x32.png") == (32, 32)
    assert png_size("src-tauri/icons/128x128.png") == (128, 128)
    assert (ROOT / "src-tauri/icons/icon.icns").stat().st_size > 10_000
    assert (ROOT / "src-tauri/icons/icon.ico").stat().st_size > 10_000

    windows = read(".github/workflows/yeschoy-windows-internal.yml")
    assert "workflow_dispatch:" in windows
    assert "--bundles nsis" in windows
    assert "signed = $false" in windows
    assert "Upload internal artifact only" in windows
    assert "release" not in windows.split("permissions:", 1)[1].split("jobs:", 1)[0]
