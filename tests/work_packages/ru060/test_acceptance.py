"""RU-060 evidence: relay receipts and child-only CLI runtime repair; never package."""
from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
MANIFEST = "src-tauri/Cargo.toml"


def run(*command: str, timeout: int = 1800, env: dict[str, str] | None = None) -> str:
    result = subprocess.run(
        command,
        cwd=ROOT,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=timeout,
        check=False,
    )
    assert result.returncode == 0, f"{' '.join(command)} failed\n{result.stdout[-12000:]}"
    return result.stdout


def source(relative: str) -> str:
    return (ROOT / relative).read_text(encoding="utf-8")


def cargo_environment(tmp_path: Path, target_name: str = "cargo-target") -> dict[str, str]:
    environment = os.environ.copy()
    target = tmp_path / target_name
    target.mkdir(exist_ok=True)
    cargo_home = tmp_path / f"{target_name}-home"
    cargo_home.mkdir(exist_ok=True)
    registry = cargo_home / "registry"
    registry.mkdir(exist_ok=True)
    original_home = Path(environment.get("CARGO_HOME", str(Path.home() / ".cargo")))
    for name in ("src", "cache", "index"):
        original = original_home / "registry" / name
        link = registry / name
        if original.exists() and not link.exists():
            link.symlink_to(original, target_is_directory=True)
    config = original_home / "config.toml"
    if config.is_file():
        shutil.copyfile(config, cargo_home / "config.toml")
    temporary = tmp_path / f"{target_name}-tmp"
    temporary.mkdir(exist_ok=True)
    environment.update(
        CARGO_HOME=str(cargo_home),
        CARGO_TARGET_DIR=str(target),
        CARGO_NET_OFFLINE="true",
        RUSTUP_TOOLCHAIN="stable",
        TMPDIR=str(temporary),
    )
    return environment


def node_modules() -> dict[str, str]:
    return json.loads(
        run(
            "node",
            "--input-type=module",
            "-e",
            "import {createRequire} from 'node:module'; "
            "const r=createRequire(import.meta.url); "
            "console.log(JSON.stringify({vitest:r.resolve('vitest/package.json'),"
            "vite:r.resolve('vite/package.json'),react:import.meta.resolve('@vitejs/plugin-react')}))",
        )
    )


def write_vite_config(path: Path, plugin: str, configuration: dict[str, object]) -> None:
    path.write_text(
        f"import react from {json.dumps(plugin)};\nexport default "
        + json.dumps(configuration).removesuffix("}")
        + ", plugins: [react()]};\n",
        encoding="utf-8",
    )


def test_native_runtime_and_dsh(tmp_path: Path) -> None:
    environment = cargo_environment(tmp_path)
    output = run(
        "cargo",
        "test",
        "--manifest-path",
        MANIFEST,
        "--locked",
        "--offline",
        "--lib",
        env=environment,
        timeout=2400,
    )
    (tmp_path / "native-tests.log").write_text(output, encoding="utf-8")
    assert re.search(r"test result: ok\. \d+ passed; 0 failed", output)
    for name in (
        "canonical_deduplication_keeps_the_first_native_wrapper",
        "cli_runtime_path_follows_wrapper_chain_without_global_mutation",
        "node_wrapper_starts_from_a_gui_safe_child_path",
        "daily_open_reuses_live_dsh_and_restarts_exited_child_without_probe",
        "latest_release_candidate_uses_the_canonical_web_profile_without_a_prompt",
    ):
        assert re.search(rf"test [^\n]*::{name} \.\.\. ok", output), name

    discovery = source("src-tauri/src/tool_discovery.rs")
    common = source("src-tauri/src/tool_adapters/common.rs")
    dsh = source("src-tauri/src/tool_adapters/dsh_web.rs")
    terminal = source("src-tauri/src/tool_adapters/terminal_launch.rs")
    assert "path,\n            location_hint" in discovery
    assert "canonical_candidates" in discovery
    assert "MAX_RUNTIME_SYMLINK_HOPS" in common
    assert "MAX_RUNTIME_PATH_DIRECTORIES" in common
    assert "apply_cli_runtime_path" in common
    assert "command.env(\"PATH\", path)" in common
    assert '[\n        "--profile",\n        "web"' in dsh
    assert "Duration::from_secs(15)" in dsh
    assert "verify_headless" not in dsh
    assert "VERIFICATION_PROMPT" not in dsh
    assert "apply_cli_runtime_path(&mut command, &installation.path)" in dsh
    assert "export PATH" in terminal
    assert "cli_runtime_directories(&installation.path)" in terminal
    for adapter in ("claude_code", "pi", "hermes", "openclaw"):
        text = source(f"src-tauri/src/tool_adapters/{adapter}.rs")
        assert "apply_cli_runtime_path(&mut command, &installation.path)" in text, adapter


def test_codex_route_evidence_ui(tmp_path: Path) -> None:
    resolved = node_modules()
    config = tmp_path / "vitest.config.mjs"
    write_vite_config(
        config,
        resolved["react"],
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
                "include": ["src/**/*.test.{ts,tsx}"],
                "exclude": ["release/**", "work/**", "tests/**", "node_modules/**"],
                "cache": {"dir": str(tmp_path / "vitest-cache")},
            },
        },
    )
    output = run(
        "node",
        str(Path(resolved["vitest"]).parent / "vitest.mjs"),
        "run",
        "--config",
        str(config),
        "--maxWorkers=2",
        "--minWorkers=1",
        "src/configuration/DailyUse.test.tsx",
        timeout=1800,
    )
    (tmp_path / "renderer-route-tests.log").write_text(output, encoding="utf-8")
    assert "43 passed" in output

    recent = source("src/configuration/RecentRequest.tsx")
    recent_normalized = " ".join(recent.split())
    view = source("src/configuration/ConfigurationPreviewView.tsx")
    codex = source("src-tauri/src/tool_adapters/codex_desktop.rs")
    diagnostics = source("src-tauri/src/request_diagnostics.rs")
    for text in (
        "已确认经野菜中转完成",
        "尚未收到该应用经野菜中转的请求",
        "官方账号是登录身份",
        "请求进入野菜本地桥后出现",
        "不依据应用缩写或 AI 的自我介绍判断",
    ):
        assert text in recent_normalized
    assert "不代表模型请求走官方计费" in view
    assert "看到完整模型 ID" in view
    assert 'document["model_provider"] = value("yeschoy")' in codex
    assert 'provider.insert("wire_api", value("responses"))' in codex
    assert 'auth.insert("command", value(helper_executable))' in codex
    assert 'assert_eq!(gpt["slug"], "gpt-6-astra")' in codex
    assert 'assert_eq!(gpt["display_name"], "GPT-6 Astra")' in codex
    assert "pub(crate) fn record(" in diagnostics
    assert "pub(crate) fn latest(" in diagnostics


def test_cross_platform_builds_without_packaging(tmp_path: Path) -> None:
    run(
        "node",
        str(ROOT / "node_modules/prettier/bin/prettier.cjs"),
        "--check",
        "src/configuration/RecentRequest.tsx",
        "src/configuration/ConfigurationPreviewView.tsx",
        "src/configuration/DailyUse.test.tsx",
        "src/workbench/AppLibraryView.tsx",
    )
    run("node", str(ROOT / "node_modules/typescript/bin/tsc"), "--noEmit")

    resolved = node_modules()
    config = tmp_path / "vite.config.mjs"
    renderer = tmp_path / "renderer"
    config.write_text(
        f"import react from {json.dumps(resolved['react'])};\n"
        "export default {"
        + f"root:{json.dumps(str(ROOT / 'src'))},base:'./',"
        + f"cacheDir:{json.dumps(str(tmp_path / 'vite-cache'))},"
        + f"build:{{outDir:{json.dumps(str(renderer))},emptyOutDir:true}},"
        + "resolve:{alias:{'@':"
        + json.dumps(str(ROOT / "src"))
        + "}},clearScreen:false,envPrefix:['VITE_','TAURI_'],plugins:[react()]};\n",
        encoding="utf-8",
    )
    run(
        "node",
        str(Path(resolved["vite"]).parent / "bin/vite.js"),
        "build",
        "--config",
        str(config),
        "--configLoader",
        "runner",
    )
    assert (renderer / "index.html").is_file()

    environment = cargo_environment(tmp_path)
    run("cargo", "fmt", "--manifest-path", MANIFEST, "--", "--check", env=environment)
    run(
        "cargo",
        "clippy",
        "--manifest-path",
        MANIFEST,
        "--locked",
        "--offline",
        "--all-targets",
        "--",
        "-D",
        "warnings",
        env=environment,
        timeout=2400,
    )
    if sys.platform == "darwin":
        run(
            "cargo",
            "check",
            "--manifest-path",
            MANIFEST,
            "--locked",
            "--offline",
            "--target",
            "aarch64-apple-darwin",
            env=environment,
            timeout=2400,
        )

    rc = tmp_path / "llvm-rc-check"
    rc.write_text(
        """#!/bin/sh
case " $* " in
  *" /? "*) printf '%s\\n' 'OVERVIEW: LLVM Resource Converter'; exit 0 ;;
esac
output_path=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "/fo" ]; then shift; output_path="$1"; break; fi
  shift
done
if [ -n "$output_path" ]; then : > "$output_path"; fi
""",
        encoding="utf-8",
    )
    rc.chmod(0o700)
    windows_env = cargo_environment(tmp_path, "cargo-windows-target")
    windows_env.update(RC=str(rc), XWIN_CROSS_COMPILER="clang")
    xwin_cache = Path.home() / "Library/Caches/cargo-xwin"
    assert (xwin_cache / "windows-msvc-sysroot").is_dir()
    isolated_xwin = tmp_path / "xwin-cache"
    run("/bin/cp", "-cR", str(xwin_cache), str(isolated_xwin), timeout=300)
    windows_env["XWIN_CACHE_DIR"] = str(isolated_xwin)
    xwin = shutil.which("cargo-xwin")
    assert xwin, "cargo-xwin is required for Windows compile evidence"
    run(
        xwin,
        "xwin",
        "check",
        "--manifest-path",
        MANIFEST,
        "--locked",
        "--offline",
        "--target",
        "x86_64-pc-windows-msvc",
        env=windows_env,
        timeout=2400,
    )
