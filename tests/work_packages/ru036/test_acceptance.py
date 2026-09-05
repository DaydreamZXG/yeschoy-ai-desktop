"""RU-036: website-aligned money, isolated full-tree regressions."""
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


def test_native(tmp_path: Path) -> None:
    output = BASE.native_regression(tmp_path)
    for name in ("website_savings_matches_recorded_ratios_and_current_rates",
                 "savings_excludes_non_comparable_charges_without_faking_zero",
                 "money_tracks_display_currency_without_a_usd_fallback",
                 "savings_rejects_invalid_envelopes_and_bounds_recent_window"):
        assert re.search(rf"test [^\n]*::{name} \.\.\. ok", output), name


def test_renderer(tmp_path: Path) -> None:
    BASE.test_renderer_daily_use_and_price_changes(tmp_path)
    result = json.loads((tmp_path / "vitest.json").read_text(encoding="utf-8"))
    for name in ("finance.test.ts", "Savings.test.tsx"):
        suite = next(item for item in result["testResults"] if Path(item["name"]).name == name)
        assert len(suite["assertionResults"]) >= 5
        assert all(item["status"] == "passed" for item in suite["assertionResults"])


def test_build(tmp_path: Path) -> None:
    BASE.test_client_build(tmp_path)
