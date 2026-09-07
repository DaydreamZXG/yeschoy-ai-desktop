"""RU-039 client privacy and startup protocol regression, isolated synthetic data."""
from __future__ import annotations

import importlib.util
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("ru035_acceptance", ROOT / "tests/work_packages/ru035/test_acceptance.py")
assert SPEC and SPEC.loader
BASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASE)


def test_native_model_privacy_and_claude_probe(tmp_path: Path) -> None:
    output = BASE.native_regression(tmp_path)
    for name in (
        "model_projection_omits_internal_descriptions_without_changing_prices",
        "claude_startup_probe_forwards_valid_empty_messages_without_verifying_activation",
        "claude_chat_probe_accepts_empty_and_thinking_only_without_fake_verification",
        "claude_proxy_preserves_useful_success_real_errors_and_auth_boundaries",
        "claude_proxy_keeps_streaming_and_rejects_sse_for_a_nonstream_request",
        "response_headers_empty_bodies_and_errors_are_not_verification",
    ):
        assert re.search(rf"test [^\n]*::{name} \.\.\. ok", output), name


def test_renderer_model_metadata_privacy(tmp_path: Path) -> None:
    BASE.test_renderer_daily_use_and_price_changes(tmp_path)
    result = json.loads((tmp_path / "vitest.json").read_text(encoding="utf-8"))
    for filename, test in (
        ("ModelPicker.test.tsx", "neither displays nor searches unreviewed upstream descriptions"),
        ("Workbench.test.tsx", "keeps raw model descriptions off both model surfaces while retaining group choices"),
    ):
        suite = next(s for s in result["testResults"] if Path(s["name"]).name == filename)
        assertion = next(a for a in suite["assertionResults"] if a["title"] == test)
        assert assertion["status"] == "passed"
