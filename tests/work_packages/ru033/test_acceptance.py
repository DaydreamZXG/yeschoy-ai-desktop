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
    env: Optional[Mapping[str, str]] = None,
) -> str:
    completed = subprocess.run(
        args,
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
        timeout=timeout,
        env=env,
    )
    return completed.stdout + completed.stderr


def rust_commands() -> tuple[str, str, str]:
    host = (
        "x86_64-apple-darwin"
        if platform.machine() == "x86_64"
        else "aarch64-apple-darwin"
    )
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


def test_codex_verification_and_failure_boundaries(tmp_path: Path) -> None:
    adapter = read("src-tauri/src/tool_adapters/codex_desktop.rs")
    activation = read("src-tauri/src/tool_activation.rs")
    bridge = read("src-tauri/src/codex_bridge.rs")
    view = read("src/configuration/ConfigurationPreviewView.tsx")

    assert "verify_credential_helper(credential).await?" in adapter
    assert "verify_provider(credential).await?" in adapter
    assert "launch(&installation.path)" in adapter
    assert "direct_responses" in adapter
    assert "codex_bridge::BASE_URL" in adapter
    assert "Policy::none()" in adapter
    assert "MAX_RESPONSE_BYTES" in adapter
    assert '"credential_helper_failed"' in adapter
    assert '"authentication_failed"' in adapter
    assert '"endpoint_unavailable"' in adapter
    assert '"provider_timed_out"' in adapter
    assert '"provider_busy"' in adapter
    assert '"model_request_rejected"' in adapter
    assert '"provider_unavailable"' in adapter
    assert '.arg("exec")' not in adapter

    assert '"/yeschoy/v1/models"' in bridge
    assert "post(responses).head(responses_head)" in bridge
    assert "secure_equal(bearer, &credential.api_key)" in bridge
    assert '"https://yeschoy.com" | "https://api.yeschoy.com"' in bridge
    assert 'TcpListener::bind(LISTEN_ADDRESS)' in bridge
    assert 'const LISTEN_ADDRESS: &str = "127.0.0.1:15722"' in bridge

    assert "prepared.rollback()" in activation
    assert "tool_credentials::restore" in activation
    assert "delete_created_token" in activation
    assert "verify_adapter(" in activation
    assert '"credential_helper_failed"' in view
    assert '"model_request_rejected"' in view
    assert '"provider_busy"' in view

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
        "tool_adapters::codex_desktop::tests",
        env=environment,
        timeout=1_800,
    )
    assert "test result: ok." in output
    assert " 0 failed" in output


def test_cache_heavy_price_summary(tmp_path: Path) -> None:
    picker = read("src/configuration/BillingGroupPicker.tsx")
    billing = read("src/configuration/billing.ts")
    styles = read("src/index.css")

    assert "1 亿 Token 费用参考" in picker
    assert "1,000 万新输入 + 8,000 万缓存读取 + 1,000 万输出" in picker
    assert "使用官网预计" in picker
    assert "使用野菜预计" in picker
    assert "NewAPI 汇率" in picker
    assert "缓存部分按输入价保守估算" in picker
    assert "实际费用以网站账单为准" in picker
    assert "价格明细" not in picker
    assert "billing-price-table" not in picker

    assert "NEW_INPUT_MILLIONS = 10" in billing
    assert "CACHE_READ_MILLIONS = 80" in billing
    assert "OUTPUT_MILLIONS = 10" in billing
    assert "group.ratio" in billing
    assert "cacheFallback" in billing
    assert ".billing-comparison-grid" in styles

    run_selected_vitest(
        [
            "src/configuration/billing.test.ts",
            "src/service-catalog/ServiceCatalogPanel.test.tsx",
        ],
        tmp_path,
    )


def test_client_only_version_build_and_release_contract(tmp_path: Path) -> None:
    package = json.loads(read("package.json"))
    tauri = json.loads(read("src-tauri/tauri.conf.json"))
    cargo = read("src-tauri/Cargo.toml")
    candidate = json.loads(read("src-tauri/tauri.candidate.conf.json"))
    workflow = read(".github/workflows/yeschoy-windows-internal.yml")

    assert package["version"] == "0.4.5"
    assert tauri["version"] == "0.4.5"
    assert 'version = "0.4.5"' in cargo
    assert candidate["bundle"]["macOS"]["hardenedRuntime"] is True
    assert candidate["bundle"]["createUpdaterArtifacts"] is False
    assert "runs-on: windows-2022" in workflow
    assert "--bundles nsis" in workflow
    assert "signed = $false" in workflow
    assert "cargo test --locked --manifest-path src-tauri/Cargo.toml" in workflow

    modules = resolve_node_modules()
    run("node", str(ROOT / "node_modules/typescript/bin/tsc"), "--noEmit", timeout=300)
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
                    "outDir": str(tmp_path / "renderer"),
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
        str(vite_manifest.parent / "bin/vite.js"),
        "build",
        "--config",
        str(config),
        "--configLoader",
        "runner",
        timeout=600,
    )

    scoped_sources = "\n".join(
        read(path)
        for path in [
            "src-tauri/src/tool_adapters/codex_desktop.rs",
            "src-tauri/src/codex_bridge.rs",
            "src-tauri/src/tool_activation.rs",
            "src/configuration/BillingGroupPicker.tsx",
            "src/configuration/billing.ts",
        ]
    )
    assert "../new-api" not in scoped_sources
    assert "new-api/" not in scoped_sources
