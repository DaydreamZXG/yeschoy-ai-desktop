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


def test_two_line_origin_authority() -> None:
    native = read("src-tauri/src/connectivity_core.rs")
    diagnostics = read("src/diagnostics/contract.ts")
    preview = read("src/configuration/preview.ts")
    credentials = read("src-tauri/src/tool_credentials.rs")

    for source in (native, diagnostics, preview, credentials):
        assert "https://yeschoy.com" in source
        assert "https://api.yeschoy.com" in source
        assert "https://yeschoy.pro" not in source

    assert 'pub const CONNECTIVITY_LINES: [LineSpec; 2]' in native
    assert 'line_id: "mainland_optimized"' in native
    assert 'line_id: "global_accelerated"' in native
    assert native.count('root_url: "https://') == 2
    assert diagnostics.count('lineId: "') == 2
    assert preview.count('id: "global_accelerated" as const') == 1


def test_session_and_activation_regressions(tmp_path: Path) -> None:
    account = read("src-tauri/src/account_v2.rs")
    activation = read("src-tauri/src/tool_activation.rs")
    assert "CONNECTIVITY_LINES" in account
    assert "refresh_line_id" in account
    assert "only refresh_access may prove the account session itself expired" in account
    assert "tool_request_verified" in activation
    assert "fallback" not in read("src-tauri/src/connectivity_core.rs").lower()

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
    )
    assert "test result: ok. 62 passed" in output

    run_selected_vitest(
        [
            "src/account/session.test.ts",
            "src/configuration/activation.test.ts",
            "src/configuration/preview.test.ts",
            "src/diagnostics/contract.test.ts",
            "src/workbench/Workbench.test.tsx",
        ],
        tmp_path,
    )


def test_candidate_version_and_renderer_build(tmp_path: Path) -> None:
    package = json.loads(read("package.json"))
    tauri = json.loads(read("src-tauri/tauri.conf.json"))
    cargo = read("src-tauri/Cargo.toml")
    readiness = read("src/candidate/readiness.ts")

    assert package["version"] == "0.4.2"
    assert tauri["version"] == "0.4.2"
    assert 'version = "0.4.2"' in cargo
    assert 'CANDIDATE_VERSION = "0.4.2"' in readiness
    assert "automaticUpdateEnabled: false" in readiness

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
