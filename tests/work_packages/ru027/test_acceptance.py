from __future__ import annotations

import json
import os
import platform
import plistlib
import subprocess
from pathlib import Path
from typing import Dict, List, Mapping, Optional


ROOT = Path(__file__).resolve().parents[3]
TAURI = ROOT / "src-tauri"
RELEASE_DMG = ROOT / "release" / "ru027-local" / "野菜API_0.4.0_universal.dmg"


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


def test_native_verified_adapter_transactions(tmp_path: Path) -> None:
    environment = os.environ.copy()
    environment["RUSTUP_TOOLCHAIN"] = "stable"
    environment["CARGO_TARGET_DIR"] = str(tmp_path / "cargo-target")
    output = run(
        "cargo",
        "test",
        "--locked",
        "--manifest-path",
        "src-tauri/Cargo.toml",
        "--lib",
        env=environment,
    )
    assert "test result: ok. 52 passed" in output

    activation = read("src-tauri/src/tool_activation.rs")
    desktop_discovery = read("src-tauri/src/desktop_app_discovery.rs")
    adapters = {
        "claude_code": "src-tauri/src/tool_adapters/claude_code.rs",
        "claude_desktop": "src-tauri/src/tool_adapters/claude_desktop.rs",
        "codex_desktop": "src-tauri/src/tool_adapters/codex_desktop.rs",
        "pi": "src-tauri/src/tool_adapters/pi.rs",
        "dsh_web": "src-tauri/src/tool_adapters/dsh_web.rs",
    }
    for tool_id, path in adapters.items():
        assert f'"{tool_id}"' in activation
        source = read(path)
        assert "FileTransaction" in source
        assert "VerificationFailed" in source

    assert '"ready"' in activation
    assert activation.index("verify_adapter(") < activation.rindex('"ready"')
    assert "restore_after_failure(" in activation
    assert "windows_sys::Win32::Storage::FileSystem" in desktop_discovery
    assert "windows_sys::Win32::System::Diagnostics::Debug" not in desktop_discovery
    adapter_registry = read("src-tauri/src/tool_adapters/mod.rs")
    dsh_adapter = read("src-tauri/src/tool_adapters/dsh_web.rs")
    assert '"0.1.1-rc.2"' in adapter_registry
    assert '"--no-open"' in dsh_adapter


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
        timeout=300,
    )
    result = json.loads(report.read_text(encoding="utf-8"))
    assert result["success"] is True
    assert result["numTotalTests"] == result["numPassedTests"]
    assert result["numFailedTests"] == 0
    assert result["numPendingTests"] == 0


def test_five_target_beginner_journey(tmp_path: Path) -> None:
    run_selected_vitest(
        [
            "src/configuration/activation.test.ts",
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
    ):
        assert f'id: "{tool_id}"' in view
    assert "installationId" in view
    assert "completed a real response" in view
    assert "完成真实回复" in view


def test_secret_and_failure_boundaries() -> None:
    credentials = read("src-tauri/src/tool_credentials.rs")
    activation = read("src-tauri/src/tool_activation.rs")
    renderer = read("src/configuration/activation.ts")

    assert 'const SERVICE: &str = "com.yeschoy.desktop.tool-credential.v2"' in credentials
    assert "keyring::v1" in credentials
    assert "credential-helper" in credentials
    assert "api_key" not in renderer.lower()
    assert "access_token" not in renderer.lower()
    assert "secret" not in renderer.lower()
    assert "tool_credentials::restore" in activation
    assert "delete_created_token" in activation
    assert "verification_failed" in activation

    rendered_config_sources = "\n".join(
        read(path)
        for path in (
            "src-tauri/src/tool_adapters/claude_code.rs",
            "src-tauri/src/tool_adapters/claude_desktop.rs",
            "src-tauri/src/tool_adapters/codex_desktop.rs",
            "src-tauri/src/tool_adapters/pi.rs",
            "src-tauri/src/tool_adapters/dsh_web.rs",
        )
    )
    assert "YESCHOY_DSH_API_KEY" in rendered_config_sources
    assert 'format!("!{helper}")' in rendered_config_sources
    assert "apiKeyHelper" in rendered_config_sources


def test_internal_candidate_build(tmp_path: Path) -> None:
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
    run(
        "node",
        str(ROOT / "node_modules/typescript/bin/tsc"),
        "--noEmit",
        timeout=180,
    )
    run(
        "node",
        str(vite_manifest.parent / "bin/vite.js"),
        "build",
        "--config",
        str(config),
        "--configLoader",
        "runner",
        timeout=300,
    )

    package = json.loads(read("package.json"))
    tauri_config = json.loads(read("src-tauri/tauri.conf.json"))
    cargo = read("src-tauri/Cargo.toml")
    readiness = read("src/candidate/readiness.ts")
    assert package["version"] == "0.4.0"
    assert tauri_config["version"] == "0.4.0"
    assert 'version = "0.4.0"' in cargo
    assert 'CANDIDATE_VERSION = "0.4.0"' in readiness

    assert RELEASE_DMG.is_file()
    assert RELEASE_DMG.stat().st_size > 1_000_000

    if platform.system() == "Darwin":
        run(
            "spctl",
            "-a",
            "-vv",
            "-t",
            "open",
            "--context",
            "context:primary-signature",
            str(RELEASE_DMG),
        )
        run("hdiutil", "verify", str(RELEASE_DMG))
        app_info = TAURI / "target/universal-apple-darwin/release/bundle/macos/野菜API.app/Contents/Info.plist"
        with app_info.open("rb") as handle:
            assert plistlib.load(handle)["CFBundleShortVersionString"] == "0.4.0"
        binary = app_info.parent / "MacOS" / "yeschoy-desktop"
        assert set(run("lipo", "-archs", str(binary)).split()) == {"arm64", "x86_64"}
