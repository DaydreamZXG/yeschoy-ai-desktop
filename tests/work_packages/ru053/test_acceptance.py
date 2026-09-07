"""RU-053 executable domestic mirror acceptance; no production mutation."""
from __future__ import annotations

import importlib.util
import json
import re
import xml.etree.ElementTree as ET
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location(
    "ru052_download_completion",
    ROOT / "tests/work_packages/ru052/test_acceptance.py",
)
assert SPEC and SPEC.loader
PREVIOUS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PREVIOUS)
BASE = PREVIOUS.BASE


def test_server_publication_and_kill_switch(tmp_path: Path) -> None:
    report = tmp_path / "server-publication.xml"
    BASE.run(
        "python3",
        "-B",
        "-m",
        "pytest",
        "--import-mode=importlib",
        "-p",
        "no:cacheprovider",
        f"--junitxml={report}",
        "--basetemp",
        str(tmp_path / "server-tests"),
        "tests/work_packages/ru044/test_acceptance.py",
        "tests/work_packages/ru045/test_acceptance.py",
        "tests/work_packages/ru052/test_vendor_pipeline.py",
        timeout=360,
    )
    cases = ET.parse(report).getroot().findall(".//testcase")
    assert len(cases) >= 70
    assert all(
        not any(case.find(tag) is not None for tag in ("failure", "error", "skipped"))
        for case in cases
    )
    names = {case.attrib["name"] for case in cases}
    for expected in (
        "test_publisher_needs_no_approval_file_and_marks_client_native_requirement",
        "test_publisher_disable_and_reenable_are_atomic_and_keep_objects",
        "test_msix_publication_requires_exact_compiled_publisher_for_both_verification_states",
        "test_origin_serves_installers_with_client_accepted_media_types",
    ):
        assert expected in names


def test_client_catalog_and_native_trust(tmp_path: Path) -> None:
    PREVIOUS.test_native_installation(tmp_path)
    output = (tmp_path / "native-tests.log").read_text(encoding="utf-8")
    assert re.search(
        r"test [^\n]*::installer_manifest_is_closed_and_exact_vendor_bound \.\.\. ok",
        output,
    )
    source = (ROOT / "src-tauri/src/app_installation/origins.rs").read_text(
        encoding="utf-8"
    )
    assert '(2, "client_native_required")' in source
    assert "catalog::allowed_url(source, &url)" in source
    platform = (ROOT / "src-tauri/src/app_installation/platform.rs").read_text(
        encoding="utf-8"
    )
    windows = (ROOT / "src-tauri/src/app_installation/windows.ps1").read_text(
        encoding="utf-8"
    )
    for expected in ("cache::digest(file)? != hash", 'Command::new("/usr/bin/codesign")', 'Command::new("/usr/sbin/spctl")'):
        assert expected in platform
    for expected in ("AppxSignature.p7x", "YESCHOY_PACKAGE_PUBLISHER", "SignatureKind"):
        assert expected in windows


def test_beginner_source_and_fallback_ui(tmp_path: Path) -> None:
    PREVIOUS.test_renderer_installation(tmp_path)
    result = json.loads((tmp_path / "vitest.json").read_text(encoding="utf-8"))
    selected = [
        assertion
        for file in result["testResults"]
        for assertion in file["assertionResults"]
        if "ru052 truthful download source" in assertion["fullName"]
    ]
    assert len(selected) == 2 and all(row["status"] == "passed" for row in selected)
    copy = (ROOT / "src/installation/InstallationPanel.tsx").read_text(encoding="utf-8")
    assert "野菜国内加速 · 安装前校验厂商签名" in copy
    assert "不可用时自动改走厂商官网" in copy
    for forbidden in ("再分发", "许可文件", "批准文件", "legal"):
        assert forbidden not in copy


def test_cross_platform_build_inputs(tmp_path: Path) -> None:
    PREVIOUS.test_installation_build(tmp_path)
