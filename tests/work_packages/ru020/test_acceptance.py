import hashlib
import json
import platform
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
SETUP = ROOT / "tests/setupGlobals.ts"
WORKFLOW = ROOT / ".github/workflows/yeschoy-windows-internal.yml"
RECEIPT = ROOT / "outputs/yeschoy-ru020-artifacts.json"

CURRENT_RENDERER_TESTS = (
    "src/candidate/readiness.test.ts",
    "src/diagnostics/contract.test.ts",
    "src/configuration/preview.test.ts",
    "src/service-catalog/contract.test.ts",
    "src/service-catalog/access-plan.test.ts",
    "src/service-catalog/ServiceCatalogPanel.test.tsx",
    "src/service-catalog/handoff.test.ts",
    "src/billing/comparison.test.ts",
    "src/billing/CostComparison.test.tsx",
    "src/workbench/Workbench.test.tsx",
    "src/workbench/Repair.test.tsx",
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def test_windows_renderer_gate_is_current_and_closed():
    workflow = WORKFLOW.read_text(encoding="utf-8")
    setup = SETUP.read_text(encoding="utf-8")
    assert "pnpm exec vitest run" in workflow
    for selector in CURRENT_RENDERER_TESTS:
        assert (ROOT / selector).is_file()
        assert workflow.count(selector) == 1
    assert '--exclude "release/**"' in workflow
    assert '--exclude "work/**"' in workflow
    assert "pnpm test:unit" not in workflow
    assert "tests/integration/App.test.tsx" not in workflow
    assert 'typeof globalThis.window.matchMedia !== "function"' in setup
    for member in (
        "addListener",
        "removeListener",
        "addEventListener",
        "removeEventListener",
        "dispatchEvent",
    ):
        assert member in setup


def test_windows_workflow_remains_private_and_bounded():
    text = WORKFLOW.read_text(encoding="utf-8")
    assert "workflow_dispatch:" in text
    assert "runs-on: windows-2022" in text
    assert "pnpm install --frozen-lockfile" in text
    assert "cargo test --locked" in text
    assert "tauri.candidate.conf.json --bundles nsis -- --locked" in text
    assert "actions/upload-artifact@v4" in text
    assert "permissions:\n  contents: read" in text
    for forbidden in (
        "softprops/action-gh-release",
        "TAURI_SIGNING_PRIVATE_KEY",
        "createUpdaterArtifacts",
        "git tag",
        "gh release",
    ):
        assert forbidden not in text


def test_final_cross_platform_receipt():
    receipt = json.loads(RECEIPT.read_text(encoding="utf-8"))
    assert receipt["schemaVersion"] == 1
    assert receipt["releaseUnit"] == "RU-020"
    assert receipt["runtimeSource"]["releaseUnit"] == "RU-017"
    assert receipt["runtimeSource"]["digest"] == (
        "5719056743ccea57281e05f90d05e14db223c10d640c43f822b9e2972f68e956"
    )

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
    assert windows["sourceCommit"] == receipt["runtimeSource"]["packagingCommit"]
    assert windows["architecture"] == "x86_64"
    assert windows["packageType"] == "nsis"
    assert windows["codeSigned"] is False
    assert windows["workflowConclusion"] == "success"
    assert windows["smartScreenWarningExpected"] is True

    assert receipt["boundaries"]["runtimeChangedByRu020"] is False
    assert receipt["boundaries"]["serverChanged"] is False
    assert receipt["boundaries"]["publicReleaseCreated"] is False
    assert receipt["boundaries"]["updaterEnabled"] is False
