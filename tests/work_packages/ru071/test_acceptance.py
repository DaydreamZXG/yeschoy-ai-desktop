from __future__ import annotations

import importlib.util
import json
from pathlib import Path
from types import SimpleNamespace

import pytest

ROOT = Path(__file__).resolve().parents[3]


def source(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def publisher_module():
    path = ROOT / "deploy/self-update/publish.py"
    spec = importlib.util.spec_from_file_location("yeschoy_self_update_publish", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def publication_fixture(tmp_path: Path, version: str = "0.4.14"):
    root = tmp_path / "srv" / "yeschoy" / "public"
    root.mkdir(parents=True, exist_ok=True)
    health_path = root / "health.json"
    if not health_path.exists():
        health_path.write_text(
            json.dumps(
                {
                    "schemaVersion": 1,
                    "service": "yeschoy-download-origin",
                    "status": "origin_ready",
                    "thirdPartyInstallersPublished": True,
                    "updatesPublished": False,
                }
            ),
            encoding="utf-8",
        )
    inputs = tmp_path / f"inputs-{version}"
    inputs.mkdir()
    mac = inputs / f"yeschoy-{version}-macos-universal.app.tar.gz"
    win = inputs / f"yeschoy-{version}-windows-x86_64.nsis.zip"
    mac.write_bytes(b"signed-macos-updater-fixture")
    win.write_bytes(b"signed-windows-updater-fixture")
    mac_sig = inputs / f"{mac.name}.sig"
    win_sig = inputs / f"{win.name}.sig"
    mac_sig.write_text("fixture-macos-signature\n", encoding="utf-8")
    win_sig.write_text("fixture-windows-signature\n", encoding="utf-8")
    notes = inputs / "release-notes.txt"
    notes.write_text("完成签名自动更新与故障恢复。", encoding="utf-8")
    args = SimpleNamespace(
        root=str(root),
        version=version,
        notes=str(notes),
        platform=[
            f"darwin-aarch64={mac},{mac_sig}",
            f"darwin-x86_64={mac},{mac_sig}",
            f"windows-x86_64={win},{win_sig}",
        ],
    )
    return root, args


def test_happy_path_is_signed_native_and_nonblocking(tmp_path: Path) -> None:
    cargo = source("src-tauri/Cargo.toml")
    native = source("src-tauri/src/app_update.rs")
    app = source("src/App.tsx")
    settings = source("src/settings/SettingsView.tsx")
    config = json.loads(source("src-tauri/tauri.conf.json"))
    candidate = json.loads(source("src-tauri/tauri.candidate.conf.json"))

    assert 'tauri-plugin-updater = { version = "=2.9.0"' in cargo
    assert ".plugin(tauri_plugin_updater::Builder::new().build())" in source(
        "src-tauri/src/lib.rs"
    )
    assert "verify_signature" not in native  # verification stays inside the pinned Tauri plugin
    assert "update.download(" in native and "update.install(&bytes)" in native
    assert config["plugins"]["updater"]["endpoints"] == [
        "https://ergou.qzz.io/updates/stable.json"
    ]
    public_key = config["plugins"]["updater"]["pubkey"]
    assert len(public_key) > 80 and "PRIVATE" not in public_key
    assert candidate["bundle"]["createUpdaterArtifacts"] is True
    assert "<UpdateProvider>" in app and "<UpdateNotice" in app
    assert "<UpdateSettingsCard />" in settings
    assert "setTimeout(check, BACKGROUND_CHECK_DELAY_MS)" in source(
        "src/update/UpdateProvider.tsx"
    )

    module = publisher_module()
    root, args = publication_fixture(tmp_path)
    result = module.publish(args)
    assert result["status"] == "published"
    manifest = json.loads((root / "updates/stable.json").read_text())
    assert set(manifest["platforms"]) == set(module.TARGETS)
    assert all(item["signature"] for item in manifest["platforms"].values())
    assert json.loads((root / "health.json").read_text())["updatesPublished"] is True


def test_failure_is_closed_and_does_not_block_other_domains(tmp_path: Path) -> None:
    native = source("src-tauri/src/app_update.rs")
    provider = source("src/update/UpdateProvider.tsx")
    capability = json.loads(source("src-tauri/capabilities/default.json"))

    assert "ACTIVATION_LOCK" not in native
    assert "AppInstallationState" not in native
    assert "AccountV2State" not in native
    assert "vendor" not in native.lower()
    assert capability["permissions"] == ["core:default"]
    assert "CHECK_REPLY_DEADLINE_MS = 15_000" in provider
    assert 'phase: "unavailable"' in provider
    assert "setAttention(false)" in provider

    module = publisher_module()
    root, args = publication_fixture(tmp_path)
    signature_path = Path(args.platform[0].split(",", 1)[1])
    signature_path.write_text("", encoding="utf-8")
    with pytest.raises(module.PublishError, match="empty_asset|invalid_signature"):
        module.publish(args)
    assert not (root / "updates/stable.json").exists()
    assert json.loads((root / "health.json").read_text())["updatesPublished"] is False


def test_duplicate_install_is_serialized_without_duplicate_shutdown(tmp_path: Path) -> None:
    native = source("src-tauri/src/app_update.rs")
    assert "gate: tokio::sync::Mutex<()>" in native
    assert "permit.cancel_safe(state.gate.lock()).await" in native
    assert "if update.version != expected_version" in native
    assert native.count("shutdown().request_shutdown()") == 1
    assert "drop(gate);\n    drop(permit);" in native

    module = publisher_module()
    _, args = publication_fixture(tmp_path)
    module.publish(args)
    with pytest.raises(module.PublishError, match="version_must_increase"):
        module.publish(args)


def test_stale_or_downgrade_manifest_is_not_installable(tmp_path: Path) -> None:
    native = source("src-tauri/src/app_update.rs")
    assert '"stale_version"' in native
    assert "update.download_url.scheme() != \"https\"" in native
    assert "update.signature.trim().is_empty()" in native

    module = publisher_module()
    _, current = publication_fixture(tmp_path, "0.4.14")
    module.publish(current)
    _, older = publication_fixture(tmp_path, "0.4.13")
    with pytest.raises(module.PublishError, match="version_must_increase"):
        module.publish(older)


def test_channel_publication_is_atomic_and_artifacts_precede_manifest(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    module = publisher_module()
    root, args = publication_fixture(tmp_path)
    original = module.atomic_write
    writes: list[str] = []

    def observed_write(path: Path, payload: bytes, mode: int = 0o644) -> None:
        if path.name == "stable.json":
            release = root / "updates/releases/0.4.14"
            assert len(list(release.glob("*"))) == 2
        writes.append(path.name)
        original(path, payload, mode)

    monkeypatch.setattr(module, "atomic_write", observed_write)
    module.publish(args)
    assert writes[-2:] == ["stable.json", "health.json"]
    caddy = source("deploy/download-origin/Caddyfile.origin")
    assert "@stableManifest" in caddy
    assert "handle /updates/releases/*" in caddy
    assert 'Cache-Control "public, max-age=31536000, immutable"' in caddy


def test_install_failure_relaunches_current_app_and_channel_can_be_disabled(
    tmp_path: Path,
) -> None:
    native = source("src-tauri/src/app_update.rs")
    assert 'write_recovery_marker(app);\n        app.request_restart();' in native
    assert '"install_failed_restarted"' in native
    assert "drain_desktop_runtimes(app).await" in native

    module = publisher_module()
    root, args = publication_fixture(tmp_path)
    module.publish(args)
    release_files = sorted((root / "updates/releases/0.4.14").glob("*"))
    result = module.disable(SimpleNamespace(root=str(root)))
    assert result["status"] == "disabled"
    assert not (root / "updates/stable.json").exists()
    assert all(path.exists() for path in release_files)
    health = json.loads((root / "health.json").read_text())
    assert health["updatesPublished"] is False
    assert health["thirdPartyInstallersPublished"] is True
    assert list((root / "updates/disabled").glob("stable-*.json"))
