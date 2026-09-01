import hashlib
import json
import platform
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
WORKFLOW = ROOT / ".github/workflows/yeschoy-windows-internal.yml"
HANDOFF = ROOT / "outputs/野菜API-跨平台内测构建与后端边界记录.md"
RECEIPT = ROOT / "outputs/yeschoy-ru018-artifacts.json"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def test_windows_internal_workflow():
    text = WORKFLOW.read_text(encoding="utf-8")
    assert "workflow_dispatch:" in text
    assert "runs-on: windows-2022" in text
    assert "pnpm install --frozen-lockfile" in text
    assert "pnpm test:unit" in text
    assert "cargo test --locked" in text
    assert "tauri.candidate.conf.json --bundles nsis -- --locked" in text
    assert "actions/upload-artifact@v4" in text
    assert "permissions:\n  contents: read" in text
    for forbidden in (
        "release.yml",
        "softprops/action-gh-release",
        "TAURI_SIGNING_PRIVATE_KEY",
        "createUpdaterArtifacts",
        "git tag",
        "gh release",
    ):
        assert forbidden not in text


def test_backend_boundary():
    text = HANDOFF.read_text(encoding="utf-8")
    for required in (
        "v1.0.0-rc.27",
        "默认不新增数据库表",
        "user_sessions",
        "Redis，5 分钟 TTL",
        "device-authorizations",
        "billing/comparison",
        "1 USD = 6.75 CNY",
        "official_pricing_version",
        "什么时候才需要动数据库",
        "本次未修改 NewAPI、Nginx、数据库或线上配置",
    ):
        assert required in text


def test_cross_platform_artifacts():
    receipt = json.loads(RECEIPT.read_text(encoding="utf-8"))
    assert receipt["schemaVersion"] == 1
    assert receipt["releaseUnit"] == "RU-018"
    assert receipt["runtimeSource"]["releaseUnit"] == "RU-017"

    mac = receipt["artifacts"]["macosUniversal"]
    mac_path = ROOT / mac["path"]
    assert mac_path.is_file()
    assert sha256(mac_path) == mac["sha256"]
    assert mac["architectures"] == ["x86_64", "arm64"]
    assert mac["developerIdSigned"] is True
    assert mac["notarized"] is True
    assert mac["stapled"] is True
    assert mac["gatekeeperAccepted"] is True
    assert mac["intelLaunchTested"] is True
    assert mac["appleSiliconLaunchTested"] is False

    app_binary = ROOT / mac["appBinaryPath"]
    if platform.system() == "Darwin":
        architectures = subprocess.check_output(
            ["lipo", "-archs", str(app_binary)], text=True
        ).strip().split()
        assert set(architectures) == {"x86_64", "arm64"}

    windows = receipt["artifacts"]["windowsX64"]
    windows_path = ROOT / windows["path"]
    assert windows_path.is_file()
    assert sha256(windows_path) == windows["sha256"]
    assert windows["architecture"] == "x86_64"
    assert windows["packageType"] == "nsis"
    assert windows["codeSigned"] is False
    assert windows["workflowConclusion"] == "success"
    assert windows["smartScreenWarningExpected"] is True

    assert receipt["boundaries"]["serverChanged"] is False
    assert receipt["boundaries"]["publicReleaseCreated"] is False
    assert receipt["boundaries"]["updaterEnabled"] is False

