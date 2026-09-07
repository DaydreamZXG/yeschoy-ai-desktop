#!/usr/bin/env python3
"""Inspect one staged installer on its native OS; never install or publish it."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile

from sync import SyncError, digest, extract_macos_zip, inspect_package, regular, sources, utc


def run(argv: list[str]) -> bytes:
    result = subprocess.run(argv, capture_output=True, timeout=180, check=False)
    if result.returncode:
        # Don't serialize arbitrary tool output or filesystem paths as evidence.
        raise SyncError(f"native_verifier_failed_{Path(argv[0]).name}")
    return result.stdout + result.stderr


def verify_app(app: Path, source: dict, package: dict) -> None:
    run(["/usr/bin/codesign", "--verify", "--deep", "--strict", str(app)])
    details = run(["/usr/bin/codesign", "-d", "--verbose=4", str(app)]).decode(errors="replace")
    if f"TeamIdentifier={source['teamId']}\n" not in details or f"Identifier={source['identity']}\n" not in details:
        raise SyncError("wrong_native_publisher")
    run(["/usr/sbin/spctl", "--assess", "--type", "execute", str(app)])
    info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
    version = info.get("CFBundleShortVersionString", "")
    if info.get("CFBundleIdentifier") != source["identity"] or not re.fullmatch(r"\d+(?:\.\d+){1,3}", version):
        raise SyncError("wrong_native_identity")
    package.update({"identity": source["identity"], "version": version, "publisher": source["teamId"]})


def verify(path: Path, source: dict, expected_publisher: str = "") -> dict:
    regular(path)
    before = digest(path)
    package = inspect_package(path, source)
    if source["platform"] == "windows":
        if sys.platform != "win32":
            raise SyncError("windows_verifier_required")
        if not expected_publisher or package.get("publisher") != expected_publisher:
            raise SyncError("approved_publisher_required")
        signtool = shutil.which("signtool.exe")
        if not signtool:
            raise SyncError("windows_sdk_signtool_required")
        run([signtool, "verify", "/pa", "/all", str(path)])
        verifier = "windows_signtool_authenticode"
    else:
        if sys.platform != "darwin":
            raise SyncError("macos_verifier_required")
        if source["format"] == "zip":
            # Only this freshly created private directory is removed; symlinks
            # inside the validated bundle are never followed by rmtree.
            with tempfile.TemporaryDirectory(prefix="yeschoy-verify-zip-") as directory:
                app = extract_macos_zip(path, Path(directory))
                verify_app(app, source, package)
            if digest(path) != before:
                raise SyncError("installer_changed_during_verification")
            return {"schemaVersion": 1, "source": source["id"], "sha256": before, "checkedAt": utc(),
                    "package": {**package, "signature": "native_verified"}, "verifier": "macos_codesign_gatekeeper",
                    "installation": "not_tested", "published": False}
        run(["/usr/bin/hdiutil", "verify", str(path)])
        folder = Path(tempfile.mkdtemp(prefix="yeschoy-verify-"))
        mount = folder / "volume"
        mount.mkdir()
        mounted = False
        try:
            run(["/usr/bin/hdiutil", "attach", "-readonly", "-nobrowse", "-noautoopen", "-mountpoint", str(mount), str(path)])
            mounted = True
            apps = [p for p in mount.glob("*.app") if p.is_dir() and not p.is_symlink()]
            if len(apps) != 1:
                raise SyncError("ambiguous_app_bundle")
            app = apps[0]
            verify_app(app, source, package)
        finally:
            # Never recursively clean a mountpoint when detach did not succeed.
            if mounted:
                run(["/usr/bin/hdiutil", "detach", str(mount)])
            mount.rmdir()
            folder.rmdir()
        verifier = "macos_codesign_gatekeeper"
    if digest(path) != before:
        raise SyncError("installer_changed_during_verification")
    return {"schemaVersion": 1, "source": source["id"], "sha256": before, "checkedAt": utc(),
            "package": {**package, "signature": "native_verified"}, "verifier": verifier,
            "installation": "not_tested", "published": False}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True)
    parser.add_argument("--file", type=Path, required=True)
    parser.add_argument("--expected-publisher", default="", help="Windows: exact independently approved Appx Publisher, not a self-accepted value")
    args = parser.parse_args()
    try:
        source = next((s for s in sources() if s["id"] == args.source), None)
        if source is None:
            raise SyncError("unknown_source")
        print(json.dumps(verify(args.file.absolute(), source, args.expected_publisher), ensure_ascii=False, indent=2))
        return 0
    except (SyncError, OSError, subprocess.TimeoutExpired) as exc:
        error = str(exc) if isinstance(exc, SyncError) else "native_verification_unavailable"
        print(json.dumps({"signature": "not_verified", "error": error, "published": False}))
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
