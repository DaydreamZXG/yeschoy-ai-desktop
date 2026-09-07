from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import subprocess


ROOT = Path(__file__).resolve().parents[3]
VERSION = "0.4.10"
RELEASE = ROOT / "release" / "internal" / VERSION
RECEIPT = RELEASE / "release-receipt.json"


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


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def receipt() -> dict:
    payload = json.loads(RECEIPT.read_text(encoding="utf-8"))
    assert payload["schemaVersion"] == 1
    assert payload["version"] == VERSION
    return payload


def test_macos_artifact(tmp_path: Path) -> None:
    payload = receipt()
    mac = payload["artifacts"]["macosUniversal"]
    dmg = ROOT / mac["path"]
    assert dmg.is_file()
    assert sha256(dmg) == mac["sha256"]
    assert mac["signed"] is True
    assert mac["notarized"] is True
    assert mac["stapled"] is True

    run("hdiutil", "verify", str(dmg))
    run("codesign", "--verify", "--strict", "--verbose=2", str(dmg))
    run("xcrun", "stapler", "validate", str(dmg))
    run("spctl", "-a", "-t", "open", "--context", "context:primary-signature", "-vv", str(dmg))

    mount = tmp_path / "dmg"
    mount.mkdir()
    run("hdiutil", "attach", "-readonly", "-nobrowse", "-mountpoint", str(mount), str(dmg))
    try:
        app = mount / "野菜API.app"
        assert app.is_dir()
        run("codesign", "--verify", "--deep", "--strict", "--verbose=2", str(app))
        run("spctl", "-a", "-t", "exec", "-vv", str(app))
        details = run("codesign", "-d", "--verbose=4", str(app))
        assert "TeamIdentifier=BRG82P5ZB7" in details
        assert "flags=0x10000(runtime)" in details
        executable = app / "Contents" / "MacOS" / "yeschoy-desktop"
        assert set(run("lipo", "-archs", str(executable)).split()) == {"x86_64", "arm64"}
        with (app / "Contents" / "Info.plist").open("rb") as handle:
            info = plistlib.load(handle)
        assert info["CFBundleShortVersionString"] == VERSION
        assert info["CFBundleIdentifier"] == "com.yeschoy.desktop"
    finally:
        run("hdiutil", "detach", str(mount))


def test_windows_artifact_and_release_boundary() -> None:
    payload = receipt()
    windows = payload["artifacts"]["windowsX64"]
    installer = ROOT / windows["path"]
    assert installer.is_file()
    assert sha256(installer) == windows["sha256"]
    assert windows["signed"] is False
    assert windows["packageType"] == "nsis"
    assert windows["installerLanguage"] == "zh-CN"
    assert payload["published"] is False
    assert payload["updaterArtifacts"] is False
    assert re.fullmatch(r"[0-9a-f]{40}", payload["sourceCommit"])
    assert payload["windowsWorkflow"]["conclusion"] == "success"
    assert payload["windowsWorkflow"]["sourceCommit"] == payload["sourceCommit"]

    package = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))
    tauri = json.loads((ROOT / "src-tauri" / "tauri.conf.json").read_text(encoding="utf-8"))
    cargo = (ROOT / "src-tauri" / "Cargo.toml").read_text(encoding="utf-8")
    assert package["version"] == VERSION
    assert tauri["version"] == VERSION
    assert re.search(r'^version = "0\\.4\\.10"$', cargo, re.MULTILINE)
    assert tauri["bundle"]["createUpdaterArtifacts"] is False
    candidate = json.loads(
        (ROOT / "src-tauri" / "tauri.candidate.conf.json").read_text(encoding="utf-8")
    )
    assert candidate["bundle"]["createUpdaterArtifacts"] is False
    assert not list(RELEASE.rglob("*.sig"))
    assert not list(RELEASE.rglob("latest.json"))
