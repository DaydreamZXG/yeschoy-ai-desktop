"""RU-054 semantic evidence; synthetic fixtures only, no package or paid probe."""
from __future__ import annotations

import importlib.util
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]


def load(name: str, relative: str):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


MODEL = load("ru054_model_predecessor", "tests/work_packages/ru043/test_acceptance.py")
INSTALL = load("ru054_install_predecessor", "tests/work_packages/ru051/test_acceptance.py")


def test_native_retry_and_claude_routes(tmp_path: Path) -> None:
    MODEL.test_native_model_capabilities(tmp_path)
    output = (tmp_path / "native-tests.log").read_text(encoding="utf-8")
    for name in (
        "ru054_pending_retry_recovers_once_and_preserves_completed_records",
        "ru054_claude_desktop_catalog_uses_safe_routes_with_real_labels",
        "ru054_claude_code_unknown_models_declare_behavior_without_changing_ids",
        "ru054_claude_desktop_alias_routes_exact_models_and_rejects_unknown_aliases",
    ):
        assert re.search(rf"test [^\n]*::{name} \.\.\. ok", output), name


def test_renderer_retry_copy(tmp_path: Path) -> None:
    INSTALL.test_all_repository_renderer(tmp_path)
    result = json.loads((tmp_path / "full-vitest.json").read_text(encoding="utf-8"))
    matches = [
        assertion
        for suite in result["testResults"]
        for assertion in suite["assertionResults"]
        if "ru054 retry recovery" in assertion["fullName"]
    ]
    assert matches and all(item["status"] == "passed" for item in matches)


def test_build_without_packaging(tmp_path: Path) -> None:
    MODEL.test_build_model_capabilities(tmp_path)
