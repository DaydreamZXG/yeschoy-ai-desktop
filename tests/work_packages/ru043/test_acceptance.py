"""RU-043 isolated semantic acceptance; no real configuration or paid calls."""
from __future__ import annotations

import importlib.util
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("ru042_profile_helpers", ROOT / "tests/work_packages/ru042/test_acceptance.py")
assert SPEC and SPEC.loader
PREVIOUS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PREVIOUS)


def test_native_model_capabilities(tmp_path: Path) -> None:
    PREVIOUS.test_native_model_sets_and_exit(tmp_path)
    output = (tmp_path / "native-tests.log").read_text(encoding="utf-8")
    for name in (
        "ru043_registry_exact_ids_and_capabilities",
        "ru043_codex_catalog_efforts_and_unknown_defaults",
        "ru043_native_model_metadata_uses_vendor_contracts",
        "ru043_reapply_clears_model_route_overrides_without_losing_budgets",
        "ru043_unknown_chat_fields_and_reasoning_defaults_are_untouched",
        "ru043_claude_reasoning_keeps_explicit_max_and_adaptive_default",
        "ru043_responses_reasoning_reaches_chat_without_silent_downgrade",
        "ru043_chat_gateway_forwards_effort_and_keeps_model_group_and_budget",
        "ru043_pi_reapply_preserves_caps_and_restores_original_bytes",
        "ru043_openclaw_reapply_keeps_user_params_and_supported_off",
        "ru043_dsh_catalog_efforts_preserve_existing_budget",
    ):
        assert re.search(rf"test [^\n]*::{name} \.\.\. ok", output), name


def test_renderer_model_capabilities(tmp_path: Path) -> None:
    PREVIOUS.test_renderer_model_sets_and_exit(tmp_path)
    result = json.loads((tmp_path / "vitest.json").read_text(encoding="utf-8"))
    assertions = [a for item in result["testResults"] for a in item["assertionResults"] if "ru043" in a["fullName"]]
    assert len(assertions) >= 3
    assert all(a["status"] == "passed" for a in assertions)


def test_build_model_capabilities(tmp_path: Path) -> None:
    PREVIOUS.test_build_model_sets_and_exit(tmp_path)
