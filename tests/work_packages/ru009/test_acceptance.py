"""Source packaging regression; actual signed artifacts are verified separately."""
import json
from pathlib import Path
import plistlib

ROOT = Path(__file__).resolve().parents[3]


def test_bundle_metadata_has_no_external_handlers() -> None:
    path = ROOT / "src-tauri/Info.plist"
    first = path.read_bytes()
    assert plistlib.loads(first) == {}
    assert path.read_bytes() == first
    assert plistlib.loads(path.read_bytes()) == {}
    for marker in (b"CFBundleURLTypes", b"CFBundleURLSchemes",
                   b"CFBundleDocumentTypes", b"ccswitch", b"CC Switch"):
        assert marker not in first
    runtime = (ROOT / "src-tauri/src/lib.rs").read_text()
    assert "tauri_plugin_deep_link" not in runtime
    assert "register_all" not in runtime


def test_candidate_identity_and_signing_boundaries() -> None:
    base = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text())
    config = json.loads((ROOT / "src-tauri/tauri.candidate.conf.json").read_text())
    assert base["productName"] == "野菜API"
    assert base["identifier"] == "com.yeschoy.desktop"
    assert base["version"] == "0.1.0"
    assert set(config) == {"$schema", "bundle"}
    assert config["bundle"] == {
        "active": True,
        "createUpdaterArtifacts": False,
        "macOS": {"hardenedRuntime": True},
    }
    assert base["bundle"]["createUpdaterArtifacts"] is False
    assert "deep-link" not in base.get("plugins", {})
    assert "updater" not in base.get("plugins", {})
    for marker in ("BEGIN PRIVATE KEY", "BEGIN RSA PRIVATE KEY", "APPLE_PASSWORD",
                   "APPLE_CERTIFICATE", "TAURI_SIGNING_PRIVATE_KEY", '"password"'):
        assert marker not in json.dumps(config)
