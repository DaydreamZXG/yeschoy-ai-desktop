"""Validate the exact local artifact; never infer public-release readiness."""
import hashlib
import json
from pathlib import Path
import plistlib
import subprocess

ROOT = Path(__file__).resolve().parents[3]


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


SOURCE_ROOTS = ["src", "src-tauri/src", "src-tauri/icons"]
SOURCE_FILES = ["package.json", "pnpm-lock.yaml", "vite.config.ts", "tsconfig.json",
                "src-tauri/Cargo.toml", "src-tauri/Cargo.lock", "src-tauri/build.rs",
                "src-tauri/tauri.conf.json", "src-tauri/tauri.candidate.conf.json",
                "src-tauri/Info.plist"]


def source_digest() -> tuple[str, int]:
    paths = [ROOT / rel for rel in SOURCE_FILES]
    for rel in SOURCE_ROOTS:
        paths.extend(path for path in (ROOT / rel).rglob("*") if path.is_file())
    rows = []
    for path in sorted(set(paths)):
        assert path.resolve().is_relative_to(ROOT)
        rows.append(f"{path.relative_to(ROOT)}\0{sha256(path)}\n")
    return hashlib.sha256("".join(rows).encode()).hexdigest(), len(rows)


def command(*args: str) -> str:
    result = subprocess.run(args, capture_output=True, text=True, timeout=90)
    assert result.returncode == 0, result.stdout + result.stderr
    return result.stdout + result.stderr


def test_local_artifacts(tmp_path: Path, monkeypatch) -> None:
    # Keep subprocesses which honor TMPDIR inside the runner-owned temp directory.
    monkeypatch.setenv("TMPDIR", str(tmp_path))
    receipt = json.loads((ROOT / "outputs/yeschoy-ru016-artifacts.json").read_text())
    assert receipt["fullProductReady"] is False
    assert receipt["publicRelease"] is False
    assert receipt["version"] == "0.1.0"
    assert receipt["platform"] == "macos-x86_64"
    assert receipt["liveBilling"] is False and receipt["managedDshLaunch"] is False
    assert receipt["notarization"]["status"] == "Accepted"
    assert receipt["notarization"]["stapled"] is True
    artifact_root = ROOT / "release/ru016-local"
    for rel, expected in receipt["files"].items():
        path = ROOT / rel
        assert path.resolve().is_relative_to(artifact_root.resolve())
        assert path.is_file() and sha256(path) == expected
    app = ROOT / receipt["appPath"]
    dmg = ROOT / receipt["dmgPath"]
    assert app.is_dir() and dmg.is_file()
    assert app.resolve().is_relative_to(artifact_root.resolve())
    info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
    assert info["CFBundleIdentifier"] == "com.yeschoy.desktop"
    assert info["CFBundleShortVersionString"] == "0.1.0"
    assert not info.get("CFBundleURLTypes") and not info.get("CFBundleDocumentTypes")
    icon = app / "Contents/Resources" / info["CFBundleIconFile"]
    assert icon.is_file() and icon.stat().st_size > 1000
    binary = app / "Contents/MacOS" / info["CFBundleExecutable"]
    assert "x86_64" in command("/usr/bin/file", str(binary))
    assert command("/usr/bin/lipo", "-archs", str(binary)).strip() == "x86_64"
    for target in [app, dmg]:
        command("/usr/bin/codesign", "--verify", "--deep", "--strict", "--verbose=2", str(target))
        signature = command("/usr/bin/codesign", "--display", "--verbose=4", str(target))
        assert "TeamIdentifier=BRG82P5ZB7" in signature
        assert "Authority=Developer ID Application:" in signature
        assert "Timestamp=" in signature
        if target == app:
            assert "runtime" in signature
    # Stapler ignores TMPDIR and writes to the Darwin per-user temp directory.
    # Its successful checks are separate captured platform evidence, not claimed
    # as sandboxed scenario execution. Required signature checks above do execute
    # here against the exact bytes checked by those platform tools.
    platform_log = ROOT / receipt["platformValidationLog"]
    assert str(platform_log.relative_to(ROOT)) in receipt["files"]
    platform_checks = platform_log.read_text()
    assert platform_checks.count("The validate action worked!") == 2
    assert platform_checks.count("source=Notarized Developer ID") == 2
    command("/usr/bin/hdiutil", "verify", str(dmg))
    source_hash, source_count = source_digest()
    assert source_count == receipt["sourceFileCount"]
    assert source_hash == receipt["sourceDigest"], "Source changed after packaging"
    # Frozen package is allowed to package, not activate deferred capabilities.
    assert "<CostComparison />" in (ROOT / "src/workbench/AccountView.tsx").read_text()
    native = (ROOT / "src-tauri/src/lib.rs").read_text()
    assert "launch_dsh" not in native and "desktop_login" not in native
