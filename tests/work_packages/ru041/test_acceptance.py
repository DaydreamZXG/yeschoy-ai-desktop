"""Cross-application repair evidence. Tests use only synthetic local fixtures.

Source stays read-only during verification. A Windows compilation is evidence
of build compatibility, not a claim that an installed Windows app responded.
"""
from __future__ import annotations

import importlib.util
import json
import os
import re
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("ru035_repair_helpers", ROOT / "tests/work_packages/ru035/test_acceptance.py")
assert SPEC and SPEC.loader
PREVIOUS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PREVIOUS)
BASE = PREVIOUS.BASE


def test_native_cross_application_regressions(tmp_path: Path) -> None:
    output = PREVIOUS.native_regression(tmp_path)
    names = (
        "diagnostics_contract_native_fixtures_match_real_serialization",
        "diagnostics_contract_request_cannot_supply_network_targets_or_secrets",
        "package_manifest_keeps_real_application_mapping_and_rejects_unsafe_paths",
        "installed_package_fixture_requires_native_application_membership_and_unambiguous_gui",
        "versioned_standalone_fixture_selects_latest_verified_gui",
        "package_dispatch_never_spawns_its_executable_or_guesses_application_id",
        "dispatch_failure_is_preserved_without_executable_fallback",
        "discovered_codex_desktop_does_not_require_an_unused_bundled_cli",
        "all_seven_open_targets_use_only_the_closed_native_request",
        "bounded_process_distinguishes_start_exit_timeout_and_output_limit",
        "bounded_process_deadline_includes_pipes_retained_after_parent_exit",
        "cancelling_bounded_process_reaps_its_owned_child",
        "claude_verification_executes_json_fixture_and_rejects_echo_error_and_failed_reply",
        "pi_verification_executes_fixture_and_rejects_banner_echo_and_failed_reply",
        "hermes_verification_executes_oneshot_fixture_and_rejects_banner_echo_and_failed_reply",
        "openclaw_verification_executes_json_fixture_and_rejects_echo_error_and_failed_reply",
        "dsh_verification_executes_headless_fixture_and_rejects_prompt_echo",
        "claude_existing_validation_is_read_only_and_checks_all_owned_model_fields",
        "pi_existing_validation_is_read_only_and_checks_both_owned_files",
        "hermes_existing_validation_preserves_config_and_rejects_changed_key_command",
        "openclaw_existing_validation_is_read_only_and_checks_full_secret_invocation",
        "restoration_errors_never_masquerade_as_initial_storage_failure",
        "terminal_plans_are_closed_and_propagate_injected_launcher_failure",
        "windows_terminal_plan_preserves_literal_unicode_and_shell_metacharacters",
        "mac_shell_command_executes_only_the_literal_fixture_path_in_home",
        "mac_command_launch_failure_cleans_private_files_after_injected_process_exit",
        "model_projection_omits_internal_descriptions_without_changing_prices",
        "claude_chat_probe_accepts_empty_and_thinking_only_without_fake_verification",
    )
    for name in names:
        assert re.search(rf"test [^\n]*::{name} \.\.\. ok", output), name
    (tmp_path / "native-tests.log").write_text(output, encoding="utf-8")

    # Reuse the private build/registry copy, never unlock the caller's cache.
    env = os.environ.copy()
    env.update(CARGO_HOME=str(tmp_path / "cargo-home"), CARGO_TARGET_DIR=str(tmp_path / "cargo-target"))
    args = ("--manifest-path", "src-tauri/Cargo.toml", "--locked", "--offline", "--all-targets")
    if sys.platform == "darwin":
        xwin = shutil.which("cargo-xwin")
        assert xwin, "Provide cargo-xwin and clang in PATH for Windows compile evidence"
        original_cache = Path(env.get("RU041_XWIN_CACHE", str(Path.home() / "Library/Caches/cargo-xwin")))
        assert (original_cache / "windows-msvc-sysroot").is_dir()
        cache = tmp_path / "xwin-cache"
        BASE.run("/bin/cp", "-cR", str(original_cache), str(cache), timeout=300)
        env.update(RUSTUP_TOOLCHAIN="1.94.0", XWIN_CROSS_COMPILER="clang", XWIN_CACHE_DIR=str(cache))
        windows = BASE.run(xwin, "xwin", "clippy", "--target", "x86_64-pc-windows-msvc", *args,
                           "--", "-D", "warnings", env=env, timeout=1500)
    else:
        assert sys.platform == "win32", "Windows compile runner is required"
        windows = BASE.run("cargo", "clippy", *args, "--", "-D", "warnings", env=env)
    (tmp_path / "windows-compile.log").write_text(windows, encoding="utf-8")


def test_renderer_diagnostics_opening_and_recovery(tmp_path: Path) -> None:
    BASE.test_renderer_login_and_reversible_workbench(tmp_path)
    result = json.loads((tmp_path / "vitest.json").read_text(encoding="utf-8"))
    required = {
        "src/diagnostics/contract.test.ts",
        "src/diagnostics/DiagnosticsView.test.tsx",
        "src/configuration/DailyUse.test.tsx",
        "src/configuration/ConfigurationPreviewView.test.tsx",
        "src/configuration/ModelPicker.test.tsx",
        "src/workbench/Workbench.test.tsx",
    }
    seen = set()
    for item in result["testResults"]:
        relative = Path(item["name"]).relative_to(ROOT).as_posix()
        if relative in required:
            seen.add(relative)
            assert item["assertionResults"]
            assert all(assertion["status"] == "passed" for assertion in item["assertionResults"])
    assert seen == required


def test_repair_version_and_production_build(tmp_path: Path) -> None:
    package = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))
    tauri = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
    candidate = json.loads((ROOT / "src-tauri/tauri.candidate.conf.json").read_text(encoding="utf-8"))
    cargo = (ROOT / "src-tauri/Cargo.toml").read_text(encoding="utf-8")
    lock = (ROOT / "src-tauri/Cargo.lock").read_text(encoding="utf-8")
    cargo_package = re.search(r"(?ms)^\[package\]\s*\n(.*?)(?=^\[|\Z)", cargo)
    assert cargo_package
    cargo_version = re.search(r'^version\s*=\s*"([^"]+)"', cargo_package[1], re.M)
    lock_version = re.search(r'(?m)^name = "yeschoy-desktop"\nversion = "([^"]+)"', lock)
    assert cargo_version and lock_version
    assert package["version"] == tauri["version"] == cargo_version[1] == lock_version[1] == "0.4.8"
    assert candidate["bundle"]["createUpdaterArtifacts"] is False
    assert candidate["bundle"]["macOS"]["hardenedRuntime"] is True
    BASE.run("node", str(ROOT / "node_modules/typescript/bin/tsc"), "--noEmit", timeout=300)
    resolved = BASE.modules()
    config = tmp_path / "vite.config.mjs"
    out = tmp_path / "renderer"
    BASE.write_config(config, resolved["react"], {
        "root": str(ROOT / "src"), "base": "./", "cacheDir": str(tmp_path / "vite-cache"),
        "build": {"outDir": str(out), "emptyOutDir": True},
        "resolve": {"alias": {"@": str(ROOT / "src")}},
        "clearScreen": False, "envPrefix": ["VITE_", "TAURI_"],
    })
    BASE.run("node", str(Path(resolved["vite"]).parent / "bin/vite.js"), "build",
             "--config", str(config), "--configLoader", "runner", timeout=600)
    assert "野菜API 桌面助手" in (out / "index.html").read_text(encoding="utf-8")
    assets = list((out / "assets").glob("*.js"))
    assert assets
    contents = [asset.read_text(encoding="utf-8") for asset in assets]
    assert any('"0.4.8"' in content for content in contents)
    for content in contents:
        assert "本地设计预览" not in content
        assert "Not available in visual fixture" not in content
