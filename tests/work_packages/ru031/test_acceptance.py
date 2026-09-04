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


def test_codex_model_visibility_and_routing(tmp_path: Path) -> None:
    view = read("src/configuration/ConfigurationPreviewView.tsx")
    compatibility = read("src/configuration/modelCompatibility.ts")
    activation = read("src-tauri/src/tool_activation.rs")
    bridge = read("src-tauri/src/codex_bridge.rs")

    assert 'includes("openai-response") || endpoints.includes("openai")' in compatibility
    assert "automaticCompatibility" in view
    assert 'Some("openai-response")' in activation
    assert "CodexTransport::DirectResponses" in activation
    assert 'Some("openai")' in activation
    assert "CodexTransport::ChatBridge" in activation
    assert "responses_to_chat_completions_with_reasoning" in bridge
    assert "create_responses_sse_stream_from_chat_with_context" in bridge
    assert "record_responses_sse_stream" in bridge

    run_selected_vitest(
        [
            "src/configuration/ConfigurationPreviewView.test.tsx",
            "src/configuration/activation.test.ts",
        ],
        tmp_path,
    )


def test_codex_catalog_secret_and_rollback_boundaries(tmp_path: Path) -> None:
    adapter = read("src-tauri/src/tool_adapters/codex_desktop.rs")
    credentials = read("src-tauri/src/tool_credentials.rs")
    bridge = read("src-tauri/src/codex_bridge.rs")

    assert "model_catalog_json" in adapter
    assert "yeschoy-model-catalog.json" in adapter
    assert "FileTransaction" in adapter
    assert "transaction.rollback" in adapter
    assert "credential-helper" in adapter
    assert "api_key" not in adapter.split("#[cfg(test)]", 1)[0]
    assert "codexTransport" not in credentials
    assert "codex_transport" in credentials
    assert "tool_credentials::load(\"codex_desktop\")" in bridge
    assert "127.0.0.1" in bridge
    assert "Authorization" not in bridge

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


def test_client_only_build_and_version(tmp_path: Path) -> None:
    package = json.loads(read("package.json"))
    tauri = json.loads(read("src-tauri/tauri.conf.json"))
    cargo = read("src-tauri/Cargo.toml")
    assert package["version"] == "0.4.3"
    assert tauri["version"] == "0.4.3"
    assert 'version = "0.4.3"' in cargo

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
            "src/configuration/ConfigurationPreviewView.tsx",
            "src-tauri/src/tool_activation.rs",
            "src-tauri/src/tool_adapters/codex_desktop.rs",
            "src-tauri/src/codex_bridge.rs",
        ]
    )
    assert "../new-api" not in client_sources
