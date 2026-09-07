from __future__ import annotations

import hashlib
import json
from pathlib import Path
import plistlib
import re
import subprocess


ROOT = Path(__file__).resolve().parents[3]
VERSION = "0.4.12"
RELEASE = ROOT / "release" / "internal" / VERSION
RECEIPT = RELEASE / "release-receipt.json"
RECIPE = ROOT / "tests" / "work_packages" / "ru066" / "installer.nsi"
PINNED_MAKENSIS_SHA256 = "9b482291c76d7965a7c535ee2fddbca5f76a0e018100f6f4950ab92c25967e1d"
SOURCE_COMMIT = "df10e7a0516c4125866daff5cf91b76178c89d02"
VERIFICATION_REPORTS = [
    ROOT
    / ".product-governance"
    / "execution"
    / "reports"
    / "d5bf9ce020ea3d93.c38b9d4cd5997e1cdcab.json",
    ROOT
    / ".product-governance"
    / "execution"
    / "reports"
    / "f46a5fcb0ab15d5b.82de8495c9996cbdd428.json",
]


def run(*args: str, timeout: int = 180) -> str:
    completed = subprocess.run(
        args,
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
        timeout=timeout,
    )
    return completed.stdout + completed.stderr


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def source_state_sha256() -> str:
    roots = [
        ROOT / "src",
        ROOT / "src-tauri" / "src",
        ROOT / "src-tauri" / "icons",
        ROOT / "src-tauri" / "capabilities",
    ]
    files = [
        ROOT / "package.json",
        ROOT / "pnpm-lock.yaml",
        ROOT / "src" / "index.html",
        ROOT / "tsconfig.json",
        ROOT / "vite.config.ts",
        ROOT / "vitest.config.ts",
        ROOT / "src-tauri" / "Cargo.toml",
        ROOT / "src-tauri" / "Cargo.lock",
        ROOT / "src-tauri" / "build.rs",
        ROOT / "src-tauri" / "tauri.conf.json",
        ROOT / "src-tauri" / "tauri.candidate.conf.json",
    ]
    for directory in roots:
        if directory.is_dir():
            files.extend(path for path in directory.rglob("*") if path.is_file())
    entries: list[dict[str, str]] = []
    for path in sorted(set(files)):
        relative = path.relative_to(ROOT).as_posix()
        if path.is_symlink():
            entries.append({"path": relative, "symlink": str(path.readlink())})
        else:
            entries.append({"path": relative, "sha256": sha256(path)})
    encoded = json.dumps(entries, ensure_ascii=False, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()


def receipt() -> dict:
    payload = json.loads(RECEIPT.read_text(encoding="utf-8"))
    assert payload["schemaVersion"] == 1
    assert payload["version"] == VERSION
    assert payload["source"]["baseCommit"] == SOURCE_COMMIT
    assert payload["source"]["stateSha256"] == source_state_sha256()
    recorded_reports = payload["source"]["verificationReports"]
    assert len(recorded_reports) == len(VERIFICATION_REPORTS)
    for expected_path, recorded in zip(VERIFICATION_REPORTS, recorded_reports):
        assert recorded["path"] == str(expected_path.relative_to(ROOT))
        assert recorded["sha256"] == sha256(expected_path)
        verified = json.loads(expected_path.read_text(encoding="utf-8"))
        assert verified["passed"] is True
        assert verified["sourceDiff"] == {}
        assert recorded["runId"] == verified["runId"]
    assert payload["published"] is False
    assert payload["updaterArtifacts"] is False
    return payload


def test_macos_artifact(tmp_path: Path, monkeypatch) -> None:
    monkeypatch.setenv("TMPDIR", str(tmp_path))
    payload = receipt()
    mac = payload["artifacts"]["macosUniversal"]
    dmg = ROOT / mac["path"]
    assert dmg.is_file()
    assert sha256(dmg) == mac["sha256"]
    assert dmg.stat().st_size == mac["sizeBytes"]
    assert mac["signed"] is True
    assert mac["notarized"] is True
    assert mac["stapled"] is True
    assert mac["architectures"] == ["x86_64", "arm64"]
    assert re.fullmatch(r"[0-9a-f-]{36}", mac["appNotarySubmissionId"])
    assert re.fullmatch(r"[0-9a-f-]{36}", mac["dmgNotarySubmissionId"])

    run("hdiutil", "verify", str(dmg))
    run("codesign", "--verify", "--strict", "--verbose=2", str(dmg))
    gatekeeper = run(
        "spctl",
        "-a",
        "-t",
        "open",
        "--context",
        "context:primary-signature",
        "-vv",
        str(dmg),
    )
    assert "source=Notarized Developer ID" in gatekeeper

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


def test_windows_cross_artifact_and_release_boundary() -> None:
    payload = receipt()
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
    assert windows["buildMethod"] == "cargo-xwin+makensis"
    assert windows["target"] == "x86_64-pc-windows-msvc"
    assert windows["realWindowsSmokeTested"] is False
    description = run("file", str(executable))
    assert "PE32+ executable" in description
    assert "x86-64" in description

    assert payload["tests"] == {"renderer": 455, "native": 479}
    assert payload["checks"]["rendererUnitTests"] is True
    assert payload["checks"]["nativeUnitTests"] is True
    assert payload["checks"]["typecheck"] is True
    assert payload["checks"]["rustfmt"] is True
    assert payload["checks"]["clippy"] is True
    assert payload["checks"]["rendererProductionBuild"] is True
    assert payload["checks"]["appleSiliconCompile"] is True
    assert payload["checks"]["windowsX64Compile"] is True
    assert payload["checks"]["prettierOwnedPaths"] is True
    assert payload["nsis"]["version"] == "3.12"
    assert payload["nsis"]["bottleSha256"] == PINNED_MAKENSIS_SHA256
    assert payload["nsis"]["recipeSha256"] == sha256(RECIPE)
    recipe_text = RECIPE.read_text(encoding="utf-8")
    assert '!insertmacro MUI_LANGUAGE "SimpChinese"' in recipe_text
    assert "RequestExecutionLevel user" in recipe_text
    assert 'VIProductVersion "0.4.12.0"' in recipe_text
    assert (
        'WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\com.yeschoy.desktop" "DisplayVersion" "0.4.12"'
        in recipe_text
    )

    package = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))
    tauri = json.loads((ROOT / "src-tauri" / "tauri.conf.json").read_text(encoding="utf-8"))
    candidate = json.loads(
        (ROOT / "src-tauri" / "tauri.candidate.conf.json").read_text(encoding="utf-8")
    )
    cargo = (ROOT / "src-tauri" / "Cargo.toml").read_text(encoding="utf-8")
    cargo_lock = (ROOT / "src-tauri" / "Cargo.lock").read_text(encoding="utf-8")
    assert package["version"] == VERSION
    assert tauri["version"] == VERSION
    assert re.search(r'^version = "0\.4\.12"$', cargo, re.MULTILINE)
    assert re.search(
        r'\[\[package\]\]\nname = "yeschoy-desktop"\nversion = "0\.4\.12"',
        cargo_lock,
    )
    assert tauri["bundle"]["createUpdaterArtifacts"] is False
    assert candidate["bundle"]["createUpdaterArtifacts"] is False
    assert not list(RELEASE.rglob("*.sig"))
    assert not list(RELEASE.rglob("latest.json"))
