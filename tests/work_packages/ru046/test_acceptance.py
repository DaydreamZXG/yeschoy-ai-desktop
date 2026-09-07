"""RU-048 installer implementation; official downloads are never installed by tests.

Reuse every preceding cross-application regression plus real local HTTP installer
fixtures. Cross compilation is not evidence of clean-machine installation.
"""
from __future__ import annotations

import importlib.util
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("ru048_prior_acceptance", ROOT / "tests/work_packages/ru043/test_acceptance.py")
assert SPEC and SPEC.loader
PREVIOUS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PREVIOUS)


def test_native_installation(tmp_path: Path) -> None:
    PREVIOUS.test_native_model_capabilities(tmp_path)
    output = (tmp_path / "native-tests.log").read_text(encoding="utf-8")
    for name in (
        "installer_catalog_is_closed_and_architecture_specific",
        "installer_download_validates_exact_ranges_and_strong_replay",
        "installer_consent_is_exact_one_use_and_never_existing_or_ambiguous",
        "installer_consent_rejects_changed_full_intent_and_expired_epoch",
        "installer_cancel_and_shutdown_never_drop_an_owned_mutation",
        "installer_exclusive_destination_and_cache_links_preserve_existing_bytes",
        "installer_windows_handoff_does_not_silently_deploy_or_change_trust",
        "installer_windows_exact_manifest_target_and_immutable_handoff",
        "installer_real_http_interruption_resumes_only_matching_bytes",
        "installer_stalled_download_cancels_without_waiting_for_server",
        "installer_handoff_dismissal_revokes_consent_and_worker_failure_releases_admission",
        "installer_progress_survives_observation_without_relaunch_or_fake_connection",
    ):
        assert re.search(rf"test [^\n]*::{name} \.\.\. ok", output), name


def test_renderer_installation(tmp_path: Path) -> None:
    PREVIOUS.test_renderer_model_capabilities(tmp_path)
    result = json.loads((tmp_path / "vitest.json").read_text(encoding="utf-8"))
    selected = [a for item in result["testResults"] for a in item["assertionResults"] if "ru048 novice installer" in a["fullName"]]
    assert len(selected) >= 11
    assert all(a["status"] == "passed" for a in selected)


def test_installation_build(tmp_path: Path) -> None:
    PREVIOUS.test_build_model_capabilities(tmp_path)
