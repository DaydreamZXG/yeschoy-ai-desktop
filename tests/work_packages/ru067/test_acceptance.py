from __future__ import annotations

import hashlib
import json
from pathlib import Path
import re
import subprocess


ROOT = Path(__file__).resolve().parents[3]
VERSION = "0.4.13"
RELEASE = ROOT / "release" / "internal" / VERSION
RECEIPT = RELEASE / "release-receipt.json"
RECIPE = ROOT / "tests" / "work_packages" / "ru067" / "installer.nsi"
BROKEN_WINDOWS_RECEIPT = ROOT / "release" / "internal" / "0.4.12" / "release-receipt.json"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run(*args: str) -> str:
    completed = subprocess.run(
        args,
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
        timeout=180,
    )
    return completed.stdout + completed.stderr


def load_receipt() -> dict:
    payload = json.loads(RECEIPT.read_text(encoding="utf-8"))
    assert payload["schemaVersion"] == 1
    assert payload["version"] == VERSION
    assert payload["published"] is False
    assert payload["updaterArtifacts"] is False
    return payload


def test_release_build_and_native_exit_are_fail_safe() -> None:
    package = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))
    tauri = json.loads(
        (ROOT / "src-tauri" / "tauri.conf.json").read_text(encoding="utf-8")
    )
    cargo = (ROOT / "src-tauri" / "Cargo.toml").read_text(encoding="utf-8")
    native = (ROOT / "src-tauri" / "src" / "lib.rs").read_text(encoding="utf-8")

    assert package["version"] == VERSION
    assert tauri["version"] == VERSION
    assert tauri["build"]["frontendDist"] == "../dist"
    assert tauri["build"]["devUrl"] == "http://127.0.0.1:1420"
    assert re.search(r'^version = "0\.4\.13"$', cargo, re.MULTILINE)
    assert re.search(
        r'\[features\]\s+default = \["custom-protocol"\]\s+'
        r'custom-protocol = \["tauri/custom-protocol"\]',
        cargo,
    )
    assert (
        '#[cfg(all(not(debug_assertions), not(feature = "custom-protocol")))]'
        in native
    )
    assert 'compile_error!("release builds require the embedded frontend")' in native
    assert 'const FRONTEND_MODE: &str = "embedded-custom-protocol";' in native
    assert 'const FRONTEND_MODE: &str = "dev-server";' in native
    assert "MessageBoxW" in native
    assert "NativeCloseChoice::Exit =>" in native
    assert "begin_desktop_shutdown(window.app_handle().clone())" in native
    assert "NativeCloseChoice::Background =>" in native
    assert "background_desktop_window(window)" in native
    assert "begin_desktop_shutdown(app.clone())" in native


def test_corrected_windows_artifact_contains_renderer_and_is_distinct() -> None:
    payload = load_receipt()
    windows = payload["artifacts"]["windowsX64"]
    installer = ROOT / windows["path"]
    executable = ROOT / windows["payloadPath"]
    assert installer.is_file() and executable.is_file()
    assert sha256(installer) == windows["sha256"]
    assert sha256(executable) == windows["payloadSha256"]
    assert installer.stat().st_size == windows["sizeBytes"]
    assert executable.stat().st_size == windows["payloadSizeBytes"]
    assert windows["signed"] is False
    assert windows["packageType"] == "nsis"
    assert windows["installerLanguage"] == "zh-CN"
    assert windows["target"] == "x86_64-pc-windows-msvc"
    assert windows["frontendMode"] == "embedded-custom-protocol"
    assert windows["rendererDevServerRequired"] is False
    assert windows["nativeExitFallback"] == "windows-message-box"
    assert windows["realWindowsSmokeTested"] is False

    description = run("file", str(executable))
    assert "PE32+ executable" in description and "x86-64" in description
    executable_bytes = executable.read_bytes()
    assert b"desktop_shell frontend_mode=embedded-custom-protocol" in executable_bytes
    assert b"desktop_shell frontend_mode=dev-server" not in executable_bytes

    index = (ROOT / "dist" / "index.html").read_text(encoding="utf-8")
    assets = re.findall(r'(?:src|href)="(?:\.)?(/assets/[^"]+)"', index)
    assert len(assets) >= 2
    for asset in assets:
        assert asset.encode() in executable_bytes

    old = json.loads(BROKEN_WINDOWS_RECEIPT.read_text(encoding="utf-8"))
    assert old["version"] == "0.4.12"
    assert windows["payloadSha256"] != old["artifacts"]["windowsX64"]["payloadSha256"]
    assert payload["regression"]["supersedes"] == "0.4.12-windows"
    assert payload["regression"]["cause"] == "missing-tauri-custom-protocol"

    recipe = RECIPE.read_text(encoding="utf-8")
    assert '!insertmacro MUI_LANGUAGE "SimpChinese"' in recipe
    assert 'VIProductVersion "0.4.13.0"' in recipe
    assert 'RequestExecutionLevel user' in recipe
