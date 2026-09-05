"""RU-035 revalidates the inherited tree before implementing everyday-use UX."""
from __future__ import annotations

import importlib.util
import json
import os
import re
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("ru034_acceptance", ROOT / "tests/work_packages/ru034/test_acceptance.py")
assert SPEC and SPEC.loader
BASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASE)


def native_regression(tmp_path: Path) -> str:
    # The semantic runner permits writes only in its private temp directory.
    # Clone the build cache there; never unlock or write the original cache.
    env = os.environ.copy()
    target = tmp_path / "cargo-target"
    source = env.get("CARGO_TARGET_DIR")
    if source and Path(source).is_dir() and sys.platform == "darwin":
        BASE.run("/bin/cp", "-cR", source, str(target), timeout=300)
    else:
        target.mkdir()
    cargo_home = tmp_path / "cargo-home"
    cargo_home.mkdir()
    original_home = Path(env.get("CARGO_HOME", str(Path.home() / ".cargo")))
    # Registry source, archives and index are immutable offline inputs. Cargo
    # lock/database files are created under the isolated new home.
    (cargo_home / "registry").mkdir()
    for name in ("src", "cache", "index"):
        original = original_home / "registry" / name
        if original.exists():
            (cargo_home / "registry" / name).symlink_to(original, target_is_directory=True)
    config = original_home / "config.toml"
    if config.is_file():
        shutil.copyfile(config, cargo_home / "config.toml")
    env.update(CARGO_HOME=str(cargo_home), CARGO_TARGET_DIR=str(target), RUSTUP_TOOLCHAIN="stable")
    args = ("--manifest-path", "src-tauri/Cargo.toml", "--locked", "--offline")
    output = BASE.run("cargo", "test", *args, "--lib", env=env, timeout=1500)
    result = re.search(r"test result: ok\. (\d+) passed; 0 failed; 0 ignored;.*; 0 filtered out", output)
    assert result and int(result[1]) >= 331, output[-16000:]
    for name in ("restores_exact_original_bytes_for_all_seven_tools",
                 "pending_switch_recovers_files_written_before_or_after_crash",
                 "cancelled_authorization_cannot_install_late_credentials",
                 "upstream_redirects_never_forward_even_synthetic_credentials"):
        assert re.search(rf"test [^\n]*::{name} \.\.\. ok", output), name
    BASE.run("cargo", "check", *args, "--bin", "yeschoy-desktop", env=env)
    BASE.run("cargo", "clippy", *args, "--all-targets", "--", "-D", "warnings", env=env)
    BASE.run("cargo", "fmt", "--manifest-path", "src-tauri/Cargo.toml", "--", "--check", env=env)
    return output


def test_inherited_baseline(tmp_path: Path) -> None:
    native_regression(tmp_path)
    BASE.test_renderer_login_and_reversible_workbench(tmp_path)
    BASE.test_client_build(tmp_path)


def test_native_daily_use_and_recovery(tmp_path: Path) -> None:
    output = native_regression(tmp_path)
    expected = [
        "open_boundary_rejects_commands_paths_and_secret_fields",
        "repeated_open_keeps_live_claude_bridge_and_credential_change_restarts",
        "dsh_existing_validation_rejects_changed_destination_without_writing",
        "daily_open_rejects_changed_helper_target_without_writing",
        "account_schema_four_omits_unknown_prices_but_keeps_zero",
        "account_switch_before_session_step_never_polls_new_account_work",
        "account_switch_during_session_step_cannot_reach_tool_token_or_write",
    ]
    if os.name == "posix":
        expected.append("daily_open_reuses_live_dsh_and_restarts_exited_child_without_probe")
    if sys.platform == "darwin":
        expected.append("launch_preflight_is_read_only_and_preserves_unrelated_settings")
    for name in expected:
        assert re.search(rf"test [^\n]*::{name} \.\.\. ok", output), name


def test_renderer_daily_use_and_price_changes(tmp_path: Path) -> None:
    BASE.test_renderer_login_and_reversible_workbench(tmp_path)
    result = json.loads((tmp_path / "vitest.json").read_text(encoding="utf-8"))
    daily_use = next(item for item in result["testResults"]
                     if Path(item["name"]).name == "DailyUse.test.tsx")
    assert len(daily_use["assertionResults"]) >= 14
    assert all(item["status"] == "passed" for item in daily_use["assertionResults"])


def test_client_build(tmp_path: Path) -> None:
    BASE.test_client_build(tmp_path)
