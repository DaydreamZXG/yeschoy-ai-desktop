from __future__ import annotations

import hashlib
import importlib.util
import json
from pathlib import Path
import re
from types import SimpleNamespace

import pytest


ROOT = Path(__file__).resolve().parents[3]


def source(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def publisher_module():
    path = ROOT / "deploy/self-update/publish.py"
    spec = importlib.util.spec_from_file_location("yeschoy_ru074_publisher", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def initialize_public_root(tmp_path: Path) -> Path:
    root = tmp_path / "srv" / "yeschoy" / "public"
    root.mkdir(parents=True, exist_ok=True)
    (root / "health.json").write_text(
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
    return root


def publication_args(tmp_path: Path, root: Path, version: str) -> SimpleNamespace:
    inputs = tmp_path / f"candidate-{version}"
    inputs.mkdir()
    mac_update = inputs / f"yeschoy-{version}-macos-universal.app.tar.gz"
    win_update = inputs / f"yeschoy-{version}-windows-x86_64.nsis.zip"
    mac_installer = inputs / f"yeschoy-{version}-macos-universal-installer.dmg"
    win_installer = inputs / f"yeschoy-{version}-windows-x86_64-installer.exe"
    mac_update.write_bytes(f"signed-mac-update-{version}".encode())
    win_update.write_bytes(f"signed-win-update-{version}".encode())
    mac_installer.write_bytes(f"notarized-mac-installer-{version}".encode())
    win_installer.write_bytes(f"authenticode-win-installer-{version}".encode())
    mac_signature = inputs / f"{mac_update.name}.sig"
    win_signature = inputs / f"{win_update.name}.sig"
    mac_signature.write_text("fixture-macos-minisign\n", encoding="utf-8")
    win_signature.write_text("fixture-windows-minisign\n", encoding="utf-8")
    notes = inputs / "release-notes.txt"
    notes.write_text(f"野菜API {version} 发布候选验证。\n", encoding="utf-8")
    return SimpleNamespace(
        root=str(root),
        version=version,
        notes=str(notes),
        platform=[
            f"darwin-aarch64={mac_update},{mac_signature}",
            f"darwin-x86_64={mac_update},{mac_signature}",
            f"windows-x86_64={win_update},{win_signature}",
        ],
        installer=[
            f"macos-universal={mac_installer}",
            f"windows-x86_64={win_installer}",
        ],
    )


def external_actions_are_pinned(workflow: str) -> bool:
    actions = re.findall(r"^\s*uses:\s*([^\s#]+)", workflow, re.MULTILINE)
    return all(
        action.startswith("./")
        or re.fullmatch(r"[^@]+@[0-9a-f]{40}", action) is not None
        for action in actions
    )


def test_happy_path_closes_shared_runtime_and_release_controls(tmp_path: Path) -> None:
    module = publisher_module()
    root = initialize_public_root(tmp_path)
    result = module.publish(publication_args(tmp_path, root, "0.4.14"))

    assert result["status"] == "published"
    updater = json.loads((root / "updates/stable.json").read_text(encoding="utf-8"))
    downloads = json.loads((root / "releases/yeschoy.json").read_text(encoding="utf-8"))
    assert set(updater["platforms"]) == set(module.TARGETS)
    assert set(downloads["platforms"]) == set(module.INSTALLER_TARGETS)
    assert all(item["url"].startswith("https://ergou.qzz.io/") for item in updater["platforms"].values())
    for item in downloads["platforms"].values():
        artifact = root / item["url"].removeprefix("https://ergou.qzz.io/")
        assert artifact.is_file()
        assert item["sha256"] == hashlib.sha256(artifact.read_bytes()).hexdigest()
        assert item["size"] == artifact.stat().st_size
    assert json.loads((root / "health.json").read_text())["updatesPublished"] is True

    activation = source("src-tauri/src/tool_activation.rs")
    for tool in (
        "claude_code",
        "claude_desktop",
        "codex_desktop",
        "pi",
        "dsh_web",
        "hermes",
        "openclaw",
    ):
        assert f'"{tool}"' in activation

    candidate = source(".github/workflows/release.yml")
    promotion = source(".github/workflows/sync-r2.yml")
    probe = source("deploy/self-update/probe_public_release.py")
    assert "Get-AuthenticodeSignature" in candidate
    assert "certificateThumbprint" in candidate
    assert "codesign --verify --deep --strict" in candidate
    assert "xcrun stapler validate" in candidate
    assert "cargo-audit" in candidate
    assert "actionlint_1.7.12_linux_amd64.tar.gz" in candidate
    assert "tauri.windows-release.runtime.json" in candidate
    assert "environment: yeschoy-production" in promotion
    assert "Verify candidate run belongs to this commit" in promotion
    assert "published_artifacts" in probe
    assert 'byte_range="bytes=0-0"' in probe
    assert "actual_size == expected_size" in probe
    assert external_actions_are_pinned(candidate)
    assert external_actions_are_pinned(promotion)


def test_failure_paths_are_bounded_and_fail_closed(tmp_path: Path) -> None:
    loopback = source("src-tauri/src/loopback_http.rs")
    dsh = source("src-tauri/src/tool_adapters/dsh_web.rs")
    discovery = source("src-tauri/src/desktop_app_discovery.rs")
    recent = source("src/configuration/RecentRequest.tsx")
    assert "MAX_AI_REQUEST_BODY_BYTES: usize = 200 * 1024 * 1024" in loopback
    assert "header::CONTENT_LENGTH" in loopback
    assert "try_acquire_owned" in loopback
    assert "AiRequestAdmissionError::PayloadTooLarge" in loopback
    assert "STARTUP_OUTPUT_LIMIT: usize = 32 * 1024" in dsh
    assert "read_bounded_line(&mut reader, remaining)" in dsh
    assert "WinVerifyTrustEx" in discovery
    assert 'payload_too_large: "请求内容超过本机安全上限"' in recent
    assert 'local_busy: "本机正在处理另一条大请求"' in recent

    module = publisher_module()
    root = initialize_public_root(tmp_path)
    args = publication_args(tmp_path, root, "0.4.14")
    args.installer.pop()
    with pytest.raises(module.PublishError, match="all_supported_installers_are_required"):
        module.publish(args)
    assert not (root / "updates/stable.json").exists()
    assert not (root / "releases/yeschoy.json").exists()
    assert json.loads((root / "health.json").read_text())["updatesPublished"] is False


def test_replay_reuses_only_exact_scope_and_never_republishes_implicitly(
    tmp_path: Path,
) -> None:
    activation = source("src-tauri/src/tool_activation.rs")
    assert '"model_limits_enabled": true' in activation
    assert '"model_limits": model_ids.join(",")' in activation
    assert '"cross_group_retry": false' in activation
    assert "actual == expected" in activation
    assert "retire_after_commit" in activation
    assert "retire_superseded_tokens" in activation

    candidate = source(".github/workflows/release.yml")
    promotion = source(".github/workflows/sync-r2.yml")
    assert re.search(r"^on:\n\s+workflow_dispatch:", candidate, re.MULTILINE)
    assert re.search(r"^on:\n\s+workflow_dispatch:", promotion, re.MULTILINE)
    assert "\n  push:" not in candidate + promotion
    assert "\n  schedule:" not in candidate + promotion
    assert 'test "$CONFIRMATION" = "PUBLISH $RELEASE_VERSION"' in promotion

    module = publisher_module()
    root = initialize_public_root(tmp_path)
    args = publication_args(tmp_path, root, "0.4.14")
    module.publish(args)
    before_update = (root / "updates/stable.json").read_bytes()
    before_download = (root / "releases/yeschoy.json").read_bytes()
    with pytest.raises(module.PublishError, match="version_must_increase"):
        module.publish(args)
    assert (root / "updates/stable.json").read_bytes() == before_update
    assert (root / "releases/yeschoy.json").read_bytes() == before_download

    module.disable(SimpleNamespace(root=str(root)))
    assert not (root / "updates/stable.json").exists()
    with pytest.raises(module.PublishError, match="version_must_increase"):
        module.publish(publication_args(tmp_path, root, "0.4.13"))
    assert not (root / "updates/stable.json").exists()
    assert (root / "releases/yeschoy.json").read_bytes() == before_download


def test_stale_legacy_and_inherited_state_is_rejected_or_migrated_truthfully() -> None:
    activation = source("src-tauri/src/tool_activation.rs")
    installation = source("src/installation/InstallationPanel.tsx")
    settings = source("src/settings/SettingsView.tsx")
    updater = source("src/update/UpdateProvider.tsx")
    assert "!m.model_id.contains(',')" in activation
    assert "old key stays valid until local commit" in activation
    assert "changed_model_scope_rotates_without_deleting_the_old_key_before_commit" in activation
    assert 'current?.mode === "guided"' in installation
    assert '"官方安装指引"' in installation
    assert '"厂商原版 · 安装前验签"' in installation
    assert '{ id: "zh", label: "简体中文", coverage: "完整" }' in settings
    assert "部分接入及安裝步驟目前仍會顯示簡體中文" in settings
    assert 'window.addEventListener("focus", checkAfterReturning)' in updater

    public_surfaces = "\n".join(
        source(path)
        for path in (
            ".github/workflows/release.yml",
            ".github/workflows/sync-r2.yml",
            "SECURITY.md",
            "deploy/self-update/README.md",
        )
    )
    for inherited in ("farion1231", "ccswitch.io", "dl.ccswitch", "cc-switch-releases"):
        assert inherited not in public_surfaces.lower()


def test_race_controls_cover_large_requests_activation_and_shutdown() -> None:
    loopback = source("src-tauri/src/loopback_http.rs")
    activation = source("src-tauri/src/tool_activation.rs")
    lifecycle = source("src-tauri/src/tool_adapters/desktop_lifecycle.rs")
    shutdown = source("src-tauri/src/lib.rs")
    promotion = source(".github/workflows/sync-r2.yml")
    assert "Semaphore::new(1)" in loopback
    assert "try_acquire_owned" in loopback
    assert "ActivationOperationState" in activation
    assert "cancel_tool_activation_v1" in activation
    assert "ACTIVATION_LOCK" in activation
    assert "No cancellation point between credential publication and local file commit" in activation
    assert "NORMAL_QUIT_LIMIT: Duration = Duration::from_secs(10)" in lifecycle
    assert "WM_CLOSE" in lifecycle
    assert "drain_desktop_runtimes" in shutdown
    assert "RUNTIME_STOP_GRACE" in shutdown
    assert "/usr/bin/flock --exclusive /opt/yeschoy-download/publish.lock" in promotion


def test_recovery_restores_user_state_and_keeps_release_gates_explicit(
    tmp_path: Path,
) -> None:
    module = publisher_module()
    root = initialize_public_root(tmp_path)
    first = module.publish(publication_args(tmp_path, root, "0.4.14"))
    first_update = (root / "updates/stable.json").read_bytes()
    first_download = (root / "releases/yeschoy.json").read_bytes()
    second = module.publish(publication_args(tmp_path, root, "0.4.15"))

    rolled_back = module.rollback(
        SimpleNamespace(
            root=str(root),
            expected_sha256=second["manifestSha256"],
            restore_sha256=second["previousManifestSha256"],
            expected_download_sha256=second["downloadManifestSha256"],
            restore_download_sha256=second["previousDownloadManifestSha256"],
        )
    )
    assert rolled_back["status"] == "rolled_back"
    assert (root / "updates/stable.json").read_bytes() == first_update
    assert (root / "releases/yeschoy.json").read_bytes() == first_download
    assert json.loads((root / "health.json").read_text())["updatesPublished"] is True
    with pytest.raises(module.PublishError, match="published_manifest_changed"):
        module.rollback(
            SimpleNamespace(
                root=str(root),
                expected_sha256=second["manifestSha256"],
                restore_sha256=first["previousManifestSha256"] or "none",
                expected_download_sha256=second["downloadManifestSha256"],
                restore_download_sha256=first["previousDownloadManifestSha256"] or "none",
            )
        )

    account = source("src/workbench/AccountView.tsx")
    restore = source("src/configuration/RestoreConnection.tsx")
    promotion = source(".github/workflows/sync-r2.yml")
    assert "仅退出野菜API账户？" in account
    assert "不会改动 Codex、Claude 等应用当前的接入设置" in account
    assert "local_settings_restored_token_cleanup_pending" in restore
    assert "本机设置已恢复并可立即生效" in restore
    assert "environment: yeschoy-production" in promotion
    assert "candidate_run_id" in promotion
    assert "StrictHostKeyChecking=yes" in promotion
