"""RU-052 executable download chain acceptance, without publishing vendor bytes."""
from __future__ import annotations

import importlib.util
import json
import os
import re
import shutil
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("ru052_prior", ROOT / "tests/work_packages/ru051/test_acceptance.py")
assert SPEC and SPEC.loader
PREVIOUS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PREVIOUS)
BASE = PREVIOUS.BASE


def test_server_download_pipeline(tmp_path: Path) -> None:
    # JUnit is built into pytest and remains available when the isolated
    # verifier deliberately disables auto-loading third-party plugins.
    report = tmp_path / "vendor-pipeline.xml"
    BASE.run("python3", "-B", "-m", "pytest", "--import-mode=importlib", "-p", "no:cacheprovider",
        f"--junitxml={report}",
        "--basetemp", str(tmp_path / "server-tests"),
        "tests/work_packages/ru044/test_acceptance.py",
        "tests/work_packages/ru045/test_acceptance.py",
        "tests/work_packages/ru052/test_vendor_pipeline.py", timeout=300)
    cases = ET.parse(report).getroot().findall(".//testcase")
    assert len(cases) >= 73
    assert all(not any(case.find(tag) is not None for tag in ("failure", "error", "skipped")) for case in cases)


def test_native_installation(tmp_path: Path) -> None:
    PREVIOUS.test_native_installation(tmp_path)
    output = (tmp_path / "native-tests.log").read_text(encoding="utf-8")
    for name in (
        "installer_manifest_is_closed_and_exact_vendor_bound",
        "installer_manifest_origin_must_be_the_package_not_the_resolver",
        "installer_claude_feed_uses_only_exact_current_official_release",
        "installer_mirror_hash_failure_falls_back_to_official_without_credentials",
        "installer_good_mirror_never_contacts_blocked_official_feed",
        "installer_resume_is_bound_to_artifact_not_merely_etag",
        "installer_claude_zip_redirect_cannot_change_the_feed_release",
        "installer_redirect_cache_never_reuses_another_resource_etag",
        "installer_redirect_resume_never_appends_another_resource_etag",
        "installer_zip_rejects_ambiguous_footer_before_library_allocation",
    ):
        assert re.search(rf"test [^\n]*::{name} \.\.\. ok", output), name
    assert re.search(r"test [^\n]*zip_package::tests::[^\n]+ \.\.\. ok", output)
    if sys.platform == "darwin":
        env = os.environ.copy()
        env.update(CARGO_HOME=str(tmp_path / "cargo-home"), CARGO_TARGET_DIR=str(tmp_path / "cargo-target"),
            RUSTUP_TOOLCHAIN="1.94.0", XWIN_CROSS_COMPILER="clang", XWIN_CACHE_DIR=str(tmp_path / "xwin-cache"))
        xwin = shutil.which("cargo-xwin")
        assert xwin
        result = BASE.run(xwin, "xwin", "clippy", "--manifest-path", "src-tauri/Cargo.toml", "--target", "aarch64-pc-windows-msvc",
            "--locked", "--offline", "--all-targets", "--", "-D", "warnings", env=env, timeout=1500)
        (tmp_path / "windows-arm-compile.log").write_text(result, encoding="utf-8")


def test_renderer_installation(tmp_path: Path) -> None:
    PREVIOUS.test_renderer_installation(tmp_path)
    result = json.loads((tmp_path / "vitest.json").read_text(encoding="utf-8"))
    selected = [a for f in result["testResults"] for a in f["assertionResults"] if "ru052 truthful download source" in a["fullName"]]
    assert len(selected) == 2 and all(a["status"] == "passed" for a in selected)


def test_installation_build(tmp_path: Path) -> None:
    PREVIOUS.test_installation_build(tmp_path)
