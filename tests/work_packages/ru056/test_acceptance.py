"""RU-056 evidence: local fixtures and compile checks; never package or probe a model."""
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


def test_unified_activation_native(tmp_path: Path) -> None:
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
        timeout=1800,
    )
    (tmp_path / "native-tests.log").write_text(output, encoding="utf-8")
    assert re.search(r"test result: ok\. \d+ passed; 0 failed", output)
    for name in (
        "only_desktop_targets_are_eligible_for_reload_control",
        "request_requires_one_of_seven_exact_targets_and_selection_shape",
        "restores_exact_original_bytes_for_all_seven_tools",
        "ru042_cancelled_setup_restores_real_files_and_secret_before_runtime_shutdown",
    ):
        assert re.search(rf"test [^\n]*::{name} \.\.\. ok", output), name

    activation = source("src-tauri/src/tool_activation.rs")
    lifecycle = source("src-tauri/src/tool_adapters/desktop_lifecycle.rs")
    adapters = source("src-tauri/src/tool_adapters/mod.rs")
    configure = activation[activation.index("pub async fn configure_desktop_tool_v2") :]
    start = activation[
        activation.index("async fn start_local_adapter") : activation.index("async fn open_configured_adapter")
    ]

    assert "verify_adapter" not in activation
    assert "verify_and_launch" not in configure
    assert "verify_launch_and_keep" not in configure
    assert "verify_provider" not in configure
    assert "configuration_ready" in configure
    assert "application_running" in configure
    assert "save_work_before_restart" in configure
    assert "DesktopReloadGuard" in configure
    assert "impl Drop for DesktopReloadGuard" in activation
    assert "open_configured_adapter" in configure
    assert not any(word in start for word in ("YESCHOY_OK", "verify(", "verify_and_launch"))
    for tool in (
        "claude_code",
        "claude_desktop",
        "codex_desktop",
        "pi",
        "dsh_web",
        "hermes",
        "openclaw",
    ):
        assert tool in configure, tool

    # Activation inventory must not start five CLIs and serially consume their
    # eight-second version-probe budgets.
    assert "probe_version" not in adapters
    assert "process-free" in adapters

    assert 'matches!(tool_id, "claude_desktop" | "codex_desktop")' in lifecycle
    assert "Duration::from_secs(10)" in lifecycle
    assert "runningApplicationsWithBundleIdentifier" in lifecycle
    assert "bundleURL" in lifecycle
    assert "QueryFullProcessImageNameW" in lifecycle
    assert "EnumWindows" in lifecycle and "WM_CLOSE" in lifecycle
    assert ".terminate()" in lifecycle
    for forbidden in ("forceTerminate(", "TerminateProcess(", "taskkill", "kill -9"):
        assert forbidden not in lifecycle


def test_unified_activation_renderer(tmp_path: Path) -> None:
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
    report = tmp_path / "vitest.json"
    run(
        "node",
        str(Path(resolved["vitest"]).parent / "vitest.mjs"),
        "run",
        "--config",
        str(config),
        "--maxWorkers=2",
        "--minWorkers=1",
        "--reporter=json",
        "--outputFile",
        str(report),
        timeout=1800,
    )
    result = json.loads(report.read_text(encoding="utf-8"))
    assert result["success"]
    assert result["numFailedTests"] == result["numPendingTests"] == 0
    assert result["numPassedTests"] == result["numTotalTests"] >= 450
    assertions = [
        assertion
        for suite in result["testResults"]
        for assertion in suite["assertionResults"]
    ]
    ru056 = [item for item in assertions if "ru056" in item["fullName"]]
    assert len(ru056) >= 6 and all(item["status"] == "passed" for item in ru056)
    consent = [
        item
        for item in assertions
        if "running-app handoff" in item["fullName"]
        or "gracefully restarting a running desktop app" in item["fullName"]
    ]
    assert len(consent) == 2 and all(item["status"] == "passed" for item in consent)

    view = source("src/configuration/ConfigurationPreviewView.tsx")
    assert "不会发送测试消息" in view
    assert "不会强制结束进程" in view
    assert "正在运行的命令行会话不会被中断" in view
    assert "第一次真实请求的结果会显示" in view


def test_unified_activation_builds(tmp_path: Path) -> None:
    run(
        "node",
        str(ROOT / "node_modules/prettier/bin/prettier.cjs"),
        "--check",
        "src/configuration/activation.ts",
        "src/configuration/activation.test.ts",
        "src/configuration/ConfigurationPreviewView.tsx",
        "src/configuration/ConfigurationPreviewView.test.tsx",
        "src/configuration/DailyUse.test.tsx",
    )
    run("node", str(ROOT / "node_modules/typescript/bin/tsc"), "--noEmit")
    resolved = node_modules()
    config = tmp_path / "vite.config.mjs"
    renderer = tmp_path / "renderer"
    write_vite_config(
        config,
        resolved["react"],
        {
            "root": str(ROOT / "src"),
            "base": "./",
            "cacheDir": str(tmp_path / "vite-cache"),
            "build": {"outDir": str(renderer), "emptyOutDir": True},
            "resolve": {"alias": {"@": str(ROOT / "src")}},
            "clearScreen": False,
            "envPrefix": ["VITE_", "TAURI_"],
        },
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
        timeout=1800,
    )
    if sys.platform == "darwin":
        apple = run(
            "cargo",
            "check",
            "--manifest-path",
            MANIFEST,
            "--locked",
            "--offline",
            "--target",
            "aarch64-apple-darwin",
            env=environment,
            timeout=1800,
        )
        (tmp_path / "apple-silicon-compile.log").write_text(apple, encoding="utf-8")

    # cargo-xwin checks Windows-specific process enumeration and WM_CLOSE code.
    # cargo check does not link resources, so a no-output llvm-rc compatibility
    # shim is sufficient on this macOS evidence host and is never used to build
    # or package a distributable artifact.
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
    windows = run(
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
        timeout=1800,
    )
    (tmp_path / "windows-x64-compile.log").write_text(windows, encoding="utf-8")
