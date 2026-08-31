import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess


ROOT = Path(__file__).resolve().parents[3]
RUNTIME = Path(__file__).with_name("runtime.cjs")
CORE = ROOT / "src-tauri/src/desktop_app_discovery_core.rs"


def read(relative: str) -> str:
    return (ROOT / relative).read_text()


def run_runtime(scenario: str, minimum_checks: int) -> None:
    completed = subprocess.run(
        ["node", str(RUNTIME), scenario],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
        timeout=120,
    )
    assert completed.returncode == 0, completed.stderr
    result = json.loads(completed.stdout)
    assert result == {"scenario": scenario, "checks": result["checks"], "passed": True}
    assert result["checks"] >= minimum_checks


def rustc_binary() -> str:
    direct = Path.home() / ".rustup/toolchains/stable-x86_64-apple-darwin/bin/rustc"
    if direct.is_file():
        return str(direct)
    found = shutil.which("rustc")
    assert found is not None, "rustc is required for RU-011 core acceptance"
    return found


def test_desktop_discovery_contract_and_native_boundary(tmp_path: Path) -> None:
    native = read("src-tauri/src/desktop_app_discovery.rs").split("#[cfg(test)]")[0]
    core = read("src-tauri/src/desktop_app_discovery_core.rs").split("#[cfg(test)]")[0]
    implementation = native + core
    contract = json.loads(read(".product-governance/contracts.json"))
    desktop_contract = next(item for item in contract["contracts"] if item["id"] == "desktop-app-discovery@v1")
    assert desktop_contract["status"] == "specified"
    assert desktop_contract["outputSchema"]["properties"]["apps"]["minItems"] == 2
    assert "DESKTOP_APP_SPECS" in native
    for required in (
        "com.anthropic.claudefordesktop", "com.openai.codex", "/Applications",
        "ChatGPT.app", "Claude.app", "LOCALAPPDATA", "ProgramFiles",
        "MAX_PLIST_BYTES", "validate_request_id", "canonicalize",
    ):
        assert required in implementation
    for forbidden in (
        "Command::new", "scan_tools_read_only", "reqwest", "TcpStream", "UdpSocket",
        "std::fs::write", "File::create", "OpenOptions", "settings.json", "config.toml",
    ):
        assert forbidden not in native
    projection = native.split("struct DesktopAppProjection", 1)[1].split("impl From", 1)[0]
    assert "path" not in projection.casefold()

    binary = tmp_path / "desktop-app-discovery-core"
    compiled = subprocess.run(
        [rustc_binary(), "--edition=2021", "--test", str(CORE), "-o", str(binary)],
        cwd=ROOT,
        env=os.environ.copy(),
        capture_output=True,
        text=True,
        check=False,
        timeout=120,
    )
    assert compiled.returncode == 0, compiled.stderr
    executed = subprocess.run(
        [str(binary)], cwd=ROOT, capture_output=True, text=True, check=False, timeout=30
    )
    assert executed.returncode == 0, executed.stdout + executed.stderr
    assert "3 passed" in executed.stdout


def test_desktop_projection_freshness_and_fail_closed_states() -> None:
    run_runtime("contract", 20)
    run_runtime("ui", 12)
    home = read("src/candidate/CandidateHomeView.tsx")
    assert "latestRequest.current !== requestId" in home
    assert 'latestRequest.current = ""' in home
    assert "isDesktopAppScanResponse" in home
    assert "setScan(null)" in home
    for state in ("detected_unverified", "not_found", "multiple_installations", "unsupported_platform"):
        assert state in read("src/desktop-apps/contract.ts")


def test_desktop_first_beginner_flow_and_cli_secondary() -> None:
    app = read("src/App.tsx")
    home = read("src/candidate/CandidateHomeView.tsx")
    setup = read("src/configuration/ConfigurationPreviewView.tsx")
    assert "CandidateHomeView" in app and "initialDesktopAppId" in app
    assert home.count('id: "claude_desktop"') == 1
    assert home.count('id: "codex_desktop"') == 1
    assert "scan_desktop_apps_read_only" in home
    assert "scan_tools_read_only" not in home
    assert "onOpenTools" in home and "yeschoyDesktop.secondary.cli" in home
    assert "DESKTOP_APPLICATIONS" in setup
    assert "CONFIGURATION_TOOLS.map" not in setup
    assert "ServiceCatalogPanel" in setup
    assert "desktop-technical-details" in setup
    assert "configuration-apply-blocked" in setup
    assert all(term not in home for term in ("Base URL", "config.toml", "settings.json", "ANTHROPIC_API_KEY"))


def png_dimensions(path: Path) -> tuple[int, int]:
    data = path.read_bytes()
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    return struct.unpack(">II", data[16:24])


def test_original_brand_icon_and_bundle_wiring() -> None:
    config = json.loads(read("src-tauri/tauri.conf.json"))
    icons = config["bundle"]["icon"]
    assert icons == [
        "icons/32x32.png", "icons/128x128.png", "icons/128x128@2x.png",
        "icons/icon.icns", "icons/icon.ico",
    ]
    source = ROOT / "src/assets/brand/yeschoy-app-icon.svg"
    assert source.is_file()
    svg = source.read_text()
    for token in ('viewBox="0 0 1024 1024"', "#2E7658", "#123E31", "#F0A46B", "<path"):
        assert token in svg
    expected = {
        "src-tauri/icons/32x32.png": (32, 32),
        "src-tauri/icons/128x128.png": (128, 128),
        "src-tauri/icons/128x128@2x.png": (256, 256),
        "src-tauri/icons/icon.png": (512, 512),
    }
    for relative, dimensions in expected.items():
        assert png_dimensions(ROOT / relative) == dimensions
    assert (ROOT / "src-tauri/icons/icon.icns").stat().st_size > 10_000
    assert (ROOT / "src-tauri/icons/icon.ico").stat().st_size > 10_000
    upstream = ROOT / "src/assets/icons/app-icon.png"
    assert hashlib.sha256(upstream.read_bytes()).digest() != hashlib.sha256((ROOT / "src-tauri/icons/icon.png").read_bytes()).digest()


def key_paths(value, prefix="") -> set[str]:
    if not isinstance(value, dict):
        return set()
    paths = set()
    for key, child in value.items():
        current = f"{prefix}.{key}" if prefix else key
        paths.add(current)
        paths.update(key_paths(child, current))
    return paths


def test_locales_responsive_focus_and_reduced_motion() -> None:
    locales = [json.loads(read(f"src/i18n/locales/{name}.json")) for name in ("zh", "zh-TW", "en", "ja")]
    expected = key_paths(locales[0]["yeschoyDesktop"])
    assert all(key_paths(locale["yeschoyDesktop"]) == expected for locale in locales[1:])
    for locale in locales:
        assert locale["yeschoyDesktop"]["apps"]["claude_desktop"]["name"] == "Claude Desktop"
        assert locale["yeschoyDesktop"]["apps"]["codex_desktop"]["name"] == "Codex"
    css = read("src/index.css")
    for required in (
        "--desktop-evergreen", "--desktop-apricot", ".desktop-app-deck",
        "grid-template-columns: 208px", "@media (max-width: 720px)",
        "@media (max-width: 420px)", "@media (prefers-reduced-motion: reduce)",
        "button:focus-visible", "min-width: 320px",
    ):
        assert required in css


def test_server_secret_write_and_release_boundaries_remain_blocked() -> None:
    library = read("src-tauri/src/lib.rs")
    handlers = re.search(r"generate_handler!\s*\[([^]]+)]", library, re.S)
    assert handlers is not None
    handler_text = handlers.group(1)
    assert handler_text.count("scan_desktop_apps_read_only") == 1
    assert "read_public_service_catalog" in handler_text
    assert "check_line_connectivity_read_only" in handler_text
    assert not any(token in handler_text for token in ("login", "recharge", "apply", "install", "update"))
    home = read("src/candidate/CandidateHomeView.tsx")
    account = read("src/account/readiness.ts")
    setup = read("src/configuration/ConfigurationPreviewView.tsx")
    catalog = read("src/service-catalog/ServiceCatalogPanel.tsx")
    config = json.loads(read("src-tauri/tauri.conf.json"))
    candidate = json.loads(read("src-tauri/tauri.candidate.conf.json"))
    assert 'REMOTE_CAPABILITY_STATUS = "unavailable"' in account
    assert "6.75 CNY" in home
    assert "configuration-apply-blocked" in setup and "disabled" in setup
    assert "readCatalog" in catalog and "onClick={readCatalog}" in catalog
    assert config["bundle"]["createUpdaterArtifacts"] is False
    assert candidate["bundle"]["createUpdaterArtifacts"] is False
    combined = home + setup + read("src/desktop-apps/contract.ts") + read("src-tauri/src/desktop_app_discovery.rs")
    for forbidden in ("BEGIN PRIVATE KEY", "ANTHROPIC_API_KEY=", "OPENAI_API_KEY=", "fetch(\"/api/desktop", "write_all("):
        assert forbidden not in combined
