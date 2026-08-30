from pathlib import Path
import json
import re
import subprocess


ROOT = Path(__file__).resolve().parents[3]
PREVIEW = ROOT / "src" / "configuration" / "preview.ts"
PREVIEW_TEST = ROOT / "src" / "configuration" / "preview.test.ts"
PREVIEW_VIEW = ROOT / "src" / "configuration" / "ConfigurationPreviewView.tsx"
APP = ROOT / "src" / "App.tsx"
READINESS = ROOT / "src" / "account" / "readiness.ts"


def source(path: Path) -> str:
    return path.read_text()


def test_fixed_tool_and_line_catalogs() -> None:
    runtime_script = f"""
const module = await import({json.dumps(PREVIEW.as_uri())});
const endpoints = Object.fromEntries(
  ["claude", "codex", "opencode", "pi"].map((toolId) => [
    toolId,
    module.createConfigurationPreview({{
      requestId: `acceptance-${{toolId}}`,
      toolId,
      lineId: "global_accelerated",
    }}).protocolEndpoint,
  ]),
);
process.stdout.write(JSON.stringify({{
  tools: module.CONFIGURATION_TOOLS.map((tool) => tool.id),
  lines: module.CONFIGURATION_LINES.map((line) => [line.id, line.rootUrl]),
  endpoints,
}}));
"""
    completed = subprocess.run(
        [
            "node",
            "--experimental-strip-types",
            "--input-type=module",
            "-e",
            runtime_script,
        ],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        timeout=180,
    )
    assert completed.returncode == 0, completed.stderr
    result = json.loads(completed.stdout)
    assert result == {
        "tools": ["claude", "codex", "opencode", "pi", "dsh"],
        "lines": [
            ["mainland_optimized", "https://yeschoy.com"],
            ["global_accelerated", "https://api.yeschoy.com"],
        ],
        "endpoints": {
            "claude": "https://api.yeschoy.com",
            "codex": "https://api.yeschoy.com/v1",
            "opencode": "https://api.yeschoy.com/v1",
            "pi": "https://api.yeschoy.com/v1",
        },
    }
    preview = source(PREVIEW)
    assert re.findall(r'id: "(claude|codex|opencode|pi|dsh)" as const', preview) == [
        "claude",
        "codex",
        "opencode",
        "pi",
        "dsh",
    ]
    assert preview.index('id: "mainland_optimized"') < preview.index(
        'id: "global_accelerated"'
    )
    assert 'rootUrl: "https://yeschoy.com" as const' in preview
    assert 'rootUrl: "https://api.yeschoy.com" as const' in preview


def test_documented_endpoint_projection() -> None:
    preview = source(PREVIEW)
    runtime_test = source(PREVIEW_TEST)

    assert preview.count('protocolSuffix: "" as const') == 2
    assert preview.count('protocolSuffix: "/v1" as const') == 3
    expected = {
        "claude": "https://yeschoy.com",
        "codex": "https://yeschoy.com/v1",
        "opencode": "https://yeschoy.com/v1",
        "pi": "https://yeschoy.com/v1",
    }
    for tool_id, endpoint in expected.items():
        assert f'["{tool_id}", "{endpoint}"]' in runtime_test
    assert "`${line.rootUrl}${tool.protocolSuffix}`" in preview


def test_preview_has_no_side_effect_authority() -> None:
    owned_source = "\n".join(
        path.read_text()
        for path in sorted((ROOT / "src" / "configuration").rglob("*.ts*"))
    )
    forbidden_operations = (
        "fetch(",
        "invoke(",
        "localStorage",
        "sessionStorage",
        "document.cookie",
        "window.open",
        "@tauri-apps",
        "writeFile",
        "readFile",
        "XMLHttpRequest",
        "WebSocket(",
    )

    assert not [item for item in forbidden_operations if item in owned_source]
    for flag in (
        "networkAttempted",
        "configurationRead",
        "configurationWritten",
        "credentialAccessed",
    ):
        assert f"{flag}: false;" in owned_source
        assert f"{flag}: false," in owned_source


def test_model_and_apply_remain_truthfully_blocked() -> None:
    preview = source(PREVIEW)
    view = source(PREVIEW_VIEW)

    assert 'status: "server_catalog_required" as const' in preview
    assert 'modelId: "" as const' in preview
    assert 'status: "blocked" as const' in preview
    for blocker in (
        "desktop_backend_required",
        "server_model_catalog_required",
        "secure_credential_helper_required",
        "exact_version_allowlist_required",
    ):
        assert f'"{blocker}"' in preview
    assert 'data-testid="configuration-apply-blocked"' in view
    assert re.search(
        r'className="blocked-apply"[\s\S]{0,100}?type="button"[\s\S]{0,100}?disabled',
        view,
    )
    assert not re.search(r"\b(?:gpt|claude|glm|deepseek)-\d", view, re.I)


def test_dsh_adapter_abstains() -> None:
    preview = source(PREVIEW)
    runtime_test = source(PREVIEW_TEST)

    dsh_definition = preview.split('id: "dsh" as const', 1)[1].split("}),", 1)[0]
    assert 'targetFile: ""' in dsh_definition
    assert "ownedFields: Object.freeze([])" in dsh_definition
    assert 'endpointStatus: "withheld_unverified"' in dsh_definition
    assert '"dsh_web_adapter_required" as const' in preview
    assert 'expect(projection.protocolEndpoint).toBe("")' in runtime_test
    assert 'expect(projection.targetFile).toBe("")' in runtime_test


def test_prior_release_unit_flows_remain_available() -> None:
    app = source(APP)
    readiness = source(READINESS)

    assert app.count('invoke<ScanResponse>("scan_tools_read_only"') == 1
    assert 'useState<AppView>("account")' in app
    assert 'type AppView = "account" | "setup" | "tools"' in app
    assert 't("yeschoyConfiguration.navigation")' in app
    assert "latestRequestRef.current = requestId" in app
    assert 'ACCOUNT_READINESS_STATUS = "backend_upgrade_required"' in readiness
    assert 'networkAttempted: false' in readiness
