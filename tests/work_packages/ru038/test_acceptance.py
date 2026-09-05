"""RU-038: internal packaging compatibility, using isolated synthetic fixtures."""
from __future__ import annotations

import importlib.util
import os
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("ru035_acceptance", ROOT / "tests/work_packages/ru035/test_acceptance.py")
assert SPEC and SPEC.loader
BASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASE)


def test_native_legacy_recovery(tmp_path: Path) -> None:
    output = BASE.native_regression(tmp_path)
    expected = ["legacy_lock_only_store_does_not_require_recovery_key",
                "missing_recovery_key_never_discards_existing_or_unknown_material",
                "missing_recovery_key_requires_a_bounded_regular_operation_lock"]
    if os.name == "posix":
        expected.append("missing_recovery_key_rejects_symlinked_store_and_lock")
    for name in expected:
        assert re.search(rf"test [^\n]*::{name} \.\.\. ok", output), name
