import base64
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

import pytest

HERE = Path(__file__).parent.resolve()
MINISIGN = os.environ.get("YESCHOY_MINISIGN", str(HERE.parents[1] / "release/tooling/minisign-0.9/minisign"))


def command(*args):
    return subprocess.run([str(a) for a in args], capture_output=True, timeout=90)


@pytest.fixture
def release(tmp_path):
    root = tmp_path / "server/public"
    root.mkdir(parents=True)
    (root / "health.json").write_text(json.dumps({"schemaVersion": 1, "service": "yeschoy-download-origin", "status": "origin_ready", "thirdPartyInstallersPublished": True, "updatesPublished": False}))
    config = {"schemaVersion": 1}
    for variant in ("official", "partner"):
        key = tmp_path / f"{variant}.key"
        pub = tmp_path / f"{variant}.key.pub"
        result = command("pnpm", "exec", "tauri", "signer", "generate", "--ci", "-p", "", "-w", key)
        assert result.returncode == 0, result.stderr.decode()
        config[variant] = {"publicKey": pub.read_text().strip(), "authorizationOrigin": "https://yeschoy.com" if variant == "official" else "https://ai.yeschoy.io", "endpoint": f"https://ergou.qzz.io/updates/{variant}/stable.json"}
    (tmp_path / "channels.json").write_text(json.dumps(config))
    (tmp_path / "notes.txt").write_text("Isolated signed update fixture")

    def stage(variant, version="0.4.16", signing_variant=None):
        directory = tmp_path / f"candidate-{variant}-{version}"
        directory.mkdir()
        for suffix in ("windows-x86_64-installer.exe", "macos-universal.app.tar.gz", "macos-universal-installer.dmg"):
            asset = directory / f"yeschoy-{version}-{variant}-{suffix}"
            asset.write_bytes((variant + version + suffix).encode())
            result = command("pnpm", "exec", "tauri", "signer", "sign", "-f", tmp_path / f"{signing_variant or variant}.key", "-p", "", asset)
            assert result.returncode == 0
        return directory

    def invoke(variant, version="0.4.16", action="publish", expected=None):
        args = [sys.executable, HERE / "publish_variant.py", action, "--root", root, "--variant", variant, "--lock", tmp_path / "publish.lock"]
        if action == "publish":
            args += ["--version", version, "--candidate", tmp_path / f"candidate-{variant}-{version}", "--notes", tmp_path / "notes.txt", "--channel-config", tmp_path / "channels.json", "--minisign", MINISIGN]
        else:
            args += ["--expected-sha256", expected]
        return command(*args)
    return root, stage, invoke


def test_signed_variants_are_isolated_and_legacy_is_not_written(release):
    root, stage, invoke = release
    for variant in ("official", "partner"):
        stage(variant)
        result = invoke(variant)
        assert result.returncode == 0, result.stdout
        manifest = json.loads((root / f"updates/{variant}/stable.json").read_bytes())
        assert manifest["variant"] == variant and manifest["version"] == "0.4.16"
        for platform in manifest["platforms"].values():
            assert f"/updates/releases/{variant}/" in platform["url"]
        downloads = json.loads((root / f"releases/yeschoy-{variant}.json").read_bytes())
        permanent = {
            "windows-x86_64": root / f"releases/{variant}/yeschoy-windows-x86_64-installer.exe",
            "macos-universal": root / f"releases/{variant}/yeschoy-macos-universal-installer.dmg",
        }
        for target, path in permanent.items():
            versioned = root / downloads["platforms"][target]["url"].removeprefix("https://ergou.qzz.io/")
            assert path.read_bytes() == versioned.read_bytes()
            assert path.stat().st_ino == versioned.stat().st_ino
    assert not (root / "updates/stable.json").exists()
    health = json.loads((root / "health.json").read_bytes())
    assert health["updateChannels"] == {"official": True, "partner": True}
    assert health["thirdPartyInstallersPublished"] is True


@pytest.mark.parametrize("failure", ["wrong_key", "corrupt", "missing"])
def test_bad_candidates_never_change_heads(release, failure):
    root, stage, invoke = release
    candidate = stage("official", signing_variant="partner" if failure == "wrong_key" else None)
    asset = next(candidate.glob("*.exe"))
    if failure == "corrupt":
        asset.write_bytes(b"corrupt after signing")
    if failure == "missing":
        asset.with_name(asset.name + ".sig").unlink()
    health = (root / "health.json").read_bytes()
    assert invoke("official").returncode != 0
    assert not (root / "updates/official/stable.json").exists()
    assert (root / "health.json").read_bytes() == health


def test_replay_downgrade_and_symlink_are_rejected(release, tmp_path):
    root, stage, invoke = release
    stage("official")
    assert invoke("official").returncode == 0
    original = (root / "updates/official/stable.json").read_bytes()
    assert invoke("official").returncode != 0
    stage("official", "0.4.15")
    assert invoke("official", "0.4.15").returncode != 0
    assert (root / "updates/official/stable.json").read_bytes() == original
    stage("partner")
    outside = tmp_path / "outside"
    outside.mkdir()
    (root / "updates/partner").symlink_to(outside, target_is_directory=True)
    assert invoke("partner").returncode != 0
    assert not list(outside.iterdir())


def test_concurrent_channels_and_compare_swap_recovery(release):
    root, stage, invoke = release
    for variant in ("official", "partner"):
        stage(variant)
    with concurrent.futures.ThreadPoolExecutor(2) as pool:
        results = list(pool.map(invoke, ("official", "partner")))
    assert all(result.returncode == 0 for result in results)
    partner = (root / "updates/partner/stable.json").read_bytes()
    receipt = json.loads(results[0].stdout)
    assert invoke("official", action="rollback", expected="0" * 64).returncode != 0
    assert invoke("official", action="rollback", expected=receipt["manifestSha256"]).returncode == 0
    assert not (root / "updates/official/stable.json").exists()
    assert not (root / "releases/official/yeschoy-windows-x86_64-installer.exe").exists()
    assert not (root / "releases/official/yeschoy-macos-universal-installer.dmg").exists()
    assert (root / "updates/partner/stable.json").read_bytes() == partner
    assert json.loads((root / "health.json").read_bytes())["updateChannels"] == {"official": False, "partner": True}
    assert invoke("official").returncode != 0


def test_rollback_restores_previous_permanent_downloads(release):
    root, stage, invoke = release
    stage("official")
    assert invoke("official").returncode == 0
    windows = root / "releases/official/yeschoy-windows-x86_64-installer.exe"
    macos = root / "releases/official/yeschoy-macos-universal-installer.dmg"
    original = windows.read_bytes(), macos.read_bytes()
    stage("official", "0.4.17")
    published = invoke("official", "0.4.17")
    assert published.returncode == 0
    assert (windows.read_bytes(), macos.read_bytes()) != original
    receipt = json.loads(published.stdout)
    assert invoke("official", action="rollback", expected=receipt["manifestSha256"]).returncode == 0
    assert (windows.read_bytes(), macos.read_bytes()) == original


def test_old_routes_are_retired_even_when_old_files_exist():
    config = (HERE.parent / "download-origin/Caddyfile.origin").read_text()
    assert "@stableManifest" not in config and "@betaManifest" not in config
    for path in ("/updates/stable.json", "/updates/beta.json"):
        assert f'handle {path} {{\n\t\t\trespond "" 204' in config
    assert "@mutableResponse not path /updates/releases/*" in config
    # -Server defers a header block; no unconditional late cache override.
    assert "\t\tCache-Control no-store" not in config


def test_disable_only_withdraws_the_expected_variant_feed(release):
    root, stage, invoke = release
    for variant in ("official", "partner"):
        stage(variant)
        assert invoke(variant).returncode == 0
    feed = root / "updates/official/stable.json"
    partner = (root / "updates/partner/stable.json").read_bytes()
    downloads = (root / "releases/yeschoy-official.json").read_bytes()
    assert invoke("official", action="disable", expected="0" * 64).returncode != 0
    expected = hashlib.sha256(feed.read_bytes()).hexdigest()
    assert invoke("official", action="disable", expected=expected).returncode == 0
    assert not feed.exists()
    assert (root / "updates/partner/stable.json").read_bytes() == partner
    assert (root / "releases/yeschoy-official.json").read_bytes() == downloads
    assert json.loads((root / "health.json").read_bytes())["updateChannels"] == {"official": False, "partner": True}
    assert invoke("official").returncode != 0


def test_legacy_cli_cannot_overwrite_dual_channel_health(release):
    root, stage, invoke = release
    stage("official")
    assert invoke("official").returncode == 0
    before = (root / "health.json").read_bytes()
    result = command(sys.executable, HERE / "publish.py", "disable", "--root", root)
    assert result.returncode != 0 and b"legacy_channel_retired" in result.stdout
    assert (root / "health.json").read_bytes() == before
