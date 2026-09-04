from __future__ import annotations

import json
import os
import platform
import subprocess
from pathlib import Path
from typing import Mapping, Optional


ROOT = Path(__file__).resolve().parents[3]


def read(relative: str) -> str:
    return (ROOT / relative).read_text(encoding="utf-8")


def run(
    *args: str,
    timeout: int = 1_200,
    cwd: Path = ROOT,
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


def rust_commands() -> tuple[str, str, str]:
    host = "x86_64-apple-darwin" if platform.machine() == "x86_64" else "aarch64-apple-darwin"
    root = Path.home() / ".rustup" / "toolchains" / f"1.94.0-{host}" / "bin"
    if (root / "cargo").is_file():
        return str(root / "cargo"), str(root / "rustc"), str(root / "rustdoc")
    return "cargo", "rustc", "rustdoc"


def resolve_node_modules() -> dict[str, str]:
    output = run(
        "node",
        "--input-type=module",
        "-e",
        "import {createRequire} from 'node:module'; const require=createRequire(import.meta.url); console.log(JSON.stringify({vitest:require.resolve('vitest/package.json'),vite:require.resolve('vite/package.json'),react:import.meta.resolve('@vitejs/plugin-react')}))",
    )
    return json.loads(output)


def run_selected_vitest(selectors: list[str], tmp_path: Path) -> None:
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


def test_claude_visibility_and_protocol_routing(tmp_path: Path) -> None:
    compatibility = read("src/configuration/modelCompatibility.ts")
    view = read("src/configuration/ConfigurationPreviewView.tsx")
    activation = read("src-tauri/src/tool_activation.rs")
    bridge = read("src-tauri/src/claude_bridge.rs")
    transform = read("src-tauri/src/proxy/providers/transform.rs")
    streaming = read("src-tauri/src/proxy/providers/streaming.rs")

    assert 'endpoints.includes("anthropic")' in compatibility
    assert 'endpoints.includes("openai")' in compatibility
    assert "modelConnectionMode" in view
    assert "ClaudeTransport::DirectAnthropic" in activation
    assert "ClaudeTransport::ChatBridge" in activation
    assert "anthropic_to_openai_with_reasoning_content" in bridge
    assert "openai_to_anthropic" in bridge
    assert "create_anthropic_sse_stream" in bridge
    assert "plan_chat_tool_output_media" in transform
    assert "reasoning_effort" in transform
    assert "cache_creation_input_tokens" in streaming

    run_selected_vitest(
        ["src/configuration/ConfigurationPreviewView.test.tsx"],
        tmp_path,
    )

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
        timeout=1_800,
    )
    assert "test result: ok." in output
    assert " 0 failed" in output


def test_claude_bridge_secret_restart_and_rollback_boundaries(tmp_path: Path) -> None:
    del tmp_path
    bridge = read("src-tauri/src/claude_bridge.rs")
    activation = read("src-tauri/src/tool_activation.rs")
    credentials = read("src-tauri/src/tool_credentials.rs")
    code = read("src-tauri/src/tool_adapters/claude_code.rs")
    desktop = read("src-tauri/src/tool_adapters/claude_desktop.rs")

    assert 'const PROXY_ADDRESS: &str = "127.0.0.1:15728"' in code
    assert 'const PROXY_ADDRESS: &str = "127.0.0.1:15729"' in desktop
    assert '"https://yeschoy.com" | "https://api.yeschoy.com"' in credentials
    assert "local_gateway_token" in credentials
    assert 'Some("chat_bridge")' in credentials
    assert "record.api_key.as_str()" in credentials
    assert "credential-helper" in credentials
    assert "state.start(credential).await" in code
    assert "state.start(credential).await" in desktop
    assert "resume_if_configured" in code
    assert "resume_if_configured" in desktop
    assert "prepared.rollback()" in activation
    assert "tool_credentials::restore" in activation
    assert "delete_created_token" in activation
    assert "previous_record" in activation
    assert "TcpListener::bind(self.address)" in bridge
    assert "strip_prefix(state.prefix)" in bridge
    assert "state.local_token" in bridge
    assert "credential.api_key" in bridge


def test_client_only_build_version_and_windows_workflow(tmp_path: Path) -> None:
    package = json.loads(read("package.json"))
    tauri = json.loads(read("src-tauri/tauri.conf.json"))
    cargo = read("src-tauri/Cargo.toml")
    workflow = read(".github/workflows/yeschoy-windows-internal.yml")
    assert package["version"] == "0.4.4"
    assert tauri["version"] == "0.4.4"
    assert 'version = "0.4.4"' in cargo
    assert "runs-on: windows-2022" in workflow
    assert "--bundles nsis" in workflow
    assert "signed = $false" in workflow
    assert "cargo test --locked --manifest-path src-tauri/Cargo.toml" in workflow

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

    client_sources = "\n".join(
        read(path)
        for path in [
            "src/configuration/modelCompatibility.ts",
            "src-tauri/src/claude_bridge.rs",
            "src-tauri/src/tool_activation.rs",
            "src-tauri/src/tool_adapters/claude_code.rs",
            "src-tauri/src/tool_adapters/claude_desktop.rs",
        ]
    )
    assert "../new-api" not in client_sources
