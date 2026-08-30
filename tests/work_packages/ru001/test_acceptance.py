import json
import os
from pathlib import Path
import re
import shutil
import subprocess


ROOT = Path(__file__).resolve().parents[3]
CORE = ROOT / "src-tauri" / "src" / "tool_discovery_core.rs"


def run_core_test(test_name: str, temporary_directory: Path) -> None:
    rustc = shutil.which("rustc")
    assert rustc is not None, "rustc is required for the frozen RU-001 acceptance suite"
    binary = temporary_directory / f"tool-discovery-{test_name}"
    compile_result = subprocess.run(
        [rustc, "--edition=2021", "--test", str(CORE), "-o", str(binary)],
        cwd=ROOT,
        env=os.environ.copy(),
        check=False,
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert compile_result.returncode == 0, compile_result.stderr
    test_result = subprocess.run(
        [str(binary), f"tests::{test_name}", "--exact"],
        cwd=ROOT,
        env=os.environ.copy(),
        check=False,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert test_result.returncode == 0, test_result.stdout + test_result.stderr


def test_fixed_catalog_and_empty_state(tmp_path: Path) -> None:
    run_core_test("fixed_catalog_and_empty_state", tmp_path)


def test_detected_version_is_exact_and_read_only(tmp_path: Path) -> None:
    run_core_test("detected_version_is_exact_and_read_only", tmp_path)


def test_multiple_installations_fail_closed(tmp_path: Path) -> None:
    run_core_test("multiple_installations_fail_closed", tmp_path)


def test_probe_failure_and_timeout_are_isolated(tmp_path: Path) -> None:
    run_core_test("probe_failure_and_timeout_are_isolated", tmp_path)


def test_runtime_exposes_no_forbidden_authority() -> None:
    library_source = (ROOT / "src-tauri" / "src" / "lib.rs").read_text()
    scanner_source = (ROOT / "src-tauri" / "src" / "tool_discovery.rs").read_text()
    cargo_manifest = (ROOT / "src-tauri" / "Cargo.toml").read_text()
    capabilities = json.loads(
        (ROOT / "src-tauri" / "capabilities" / "default.json").read_text()
    )
    tauri_config = json.loads((ROOT / "src-tauri" / "tauri.conf.json").read_text())

    handler_match = re.search(r"generate_handler!\s*\[([^]]+)]", library_source, re.S)
    assert handler_match is not None
    handlers = [item.strip() for item in handler_match.group(1).split(",") if item.strip()]
    assert handlers == ["tool_discovery::scan_tools_read_only"]
    assert ".plugin(" not in library_source

    declared_dependencies = {
        line.split("=", 1)[0].strip()
        for line in cargo_manifest.split("[dependencies]", 1)[1]
        .split("[profile.release]", 1)[0]
        .splitlines()
        if "=" in line
    }
    assert declared_dependencies == {"serde", "tauri", "tokio"}
    assert capabilities["permissions"] == ["core:default"]
    assert "plugins" not in tauri_config
    assert tauri_config["bundle"]["createUpdaterArtifacts"] is False
    csp = tauri_config["app"]["security"]["csp"]
    assert "https:" not in csp
    assert "connect-src 'self' ipc: http://ipc.localhost" in csp

    forbidden_scanner_tokens = (
        "reqwest",
        "TcpStream",
        "UdpSocket",
        "std::fs::write",
        "File::create",
        "OpenOptions",
        ".credentials",
        "settings.json",
        "config.toml",
        "models.json",
        "plugin add",
    )
    assert not [token for token in forbidden_scanner_tokens if token in scanner_source]


def test_ui_rejects_stale_scan_completion() -> None:
    app_source = (ROOT / "src" / "App.tsx").read_text()
    assert "latestRequestRef.current = requestId" in app_source
    assert app_source.count("latestRequestRef.current !== requestId") >= 2
    assert 'setScan(null);\n    setPhase("loading")' in app_source
    assert "rescanButton" in app_source
    assert "configure" not in app_source.casefold()
    assert "api key" not in app_source.casefold()
