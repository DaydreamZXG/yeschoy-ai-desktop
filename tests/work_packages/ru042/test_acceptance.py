"""RU-042 semantic evidence; synthetic data only, no installer or live account.

Reuse the private read-only runner and cross-compilers, then require the new
executable regressions by name. Windows compilation is not a live-app test.
"""
from __future__ import annotations

import importlib.util
import json
import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("ru041_model_set_helpers", ROOT / "tests/work_packages/ru041/test_acceptance.py")
assert SPEC and SPEC.loader
PREVIOUS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PREVIOUS)


def test_native_model_sets_and_exit(tmp_path: Path) -> None:
    PREVIOUS.test_native_cross_application_regressions(tmp_path)
    output = (tmp_path / "native-tests.log").read_text(encoding="utf-8")
    names = (
        "ru042_activation_models_require_explicit_unique_binding_and_default_member",
        "ru042_keyring_generations_obey_windows_size_and_restore_logical_snapshot",
        "ru042_keyring_partial_write_never_publishes_incomplete_generation",
        "ru042_model_credentials_keep_exact_group_keys_and_reject_duplicates",
        "ru042_helper_read_retries_only_a_changed_keyring_generation",
        "ru042_cancelled_setup_restores_real_files_and_secret_before_runtime_shutdown",
        "ru042_chat_gateway_routes_two_groups_and_pins_inflight_snapshot",
        "ru042_chat_gateway_rejects_unknown_models_wrong_tool_auth_and_redirects",
        "ru042_chat_gateway_stream_errors_and_incomplete_json_never_succeed",
        "ru042_claude_exact_models_use_distinct_keys_and_protocols",
        "ru042_claude_route_update_keeps_listener_and_inflight_snapshot",
        "ru042_claude_chat_sse_error_is_not_lost_or_verified",
        "ru042_claude_desktop_profile_real_identity_and_default_order_are_verified",
        "ru042_claude_code_catalog_uses_real_picker_ids_and_member_defaults",
        "ru042_codex_catalog_is_conservative_and_accepts_registered_native_default",
        "ru042_pi_catalog_keeps_real_ids_and_accepts_registered_native_default",
        "ru042_hermes_catalog_defaults_change_only_within_registered_set",
        "ru042_openclaw_catalog_removes_stale_owned_aliases_and_allows_member_default",
        "ru042_dsh_catalog_update_reuses_real_fixture_process",
        "concurrent_admission_cannot_cross_the_confirmed_shutdown_gate",
        "confirmation_rejects_new_operations_and_cancels_safe_waits",
        "stalled_runtime_hooks_share_one_deadline_and_are_cancelled",
    )
    for name in names:
        assert re.search(rf"test [^\n]*::{name} \.\.\. ok", output), name
    if sys.platform == "darwin":
        env = os.environ.copy()
        env.update(CARGO_HOME=str(tmp_path / "cargo-home"), CARGO_TARGET_DIR=str(tmp_path / "cargo-target"), RUSTUP_TOOLCHAIN="stable")
        arm = PREVIOUS.BASE.run("cargo", "clippy", "--manifest-path", "src-tauri/Cargo.toml",
            "--target", "aarch64-apple-darwin", "--locked", "--offline", "--all-targets", "--", "-D", "warnings", env=env, timeout=1500)
        (tmp_path / "apple-silicon-compile.log").write_text(arm, encoding="utf-8")


def test_renderer_model_sets_and_exit(tmp_path: Path) -> None:
    PREVIOUS.test_renderer_diagnostics_opening_and_recovery(tmp_path)
    result = json.loads((tmp_path / "vitest.json").read_text(encoding="utf-8"))
    assertions = [a for item in result["testResults"] for a in item["assertionResults"]]
    selected = [a for a in assertions if "ru042" in a["fullName"]]
    assert len(selected) >= 5 and all(a["status"] == "passed" for a in selected)
    shutdown = next(item for item in result["testResults"] if Path(item["name"]).name == "QuitAssistant.test.tsx")
    assert len(shutdown["assertionResults"]) >= 13
    assert all(a["status"] == "passed" for a in shutdown["assertionResults"])


def test_build_model_sets_and_exit(tmp_path: Path) -> None:
    PREVIOUS.test_repair_version_and_production_build(tmp_path)
