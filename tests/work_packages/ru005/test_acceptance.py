import json
from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / "src" / "App.tsx"
HOME = ROOT / "src" / "candidate" / "CandidateHomeView.tsx"
READINESS = ROOT / "src" / "candidate" / "readiness.ts"
DIAGNOSTICS = ROOT / "src" / "diagnostics" / "DiagnosticsView.tsx"
DIAGNOSTIC_CONTRACT = ROOT / "src" / "diagnostics" / "contract.ts"
CONNECTIVITY = ROOT / "src-tauri" / "src" / "connectivity.rs"
CONNECTIVITY_CORE = ROOT / "src-tauri" / "src" / "connectivity_core.rs"
TAURI_LIB = ROOT / "src-tauri" / "src" / "lib.rs"
CAPABILITY = ROOT / "src-tauri" / "capabilities" / "default.json"
SETTINGS = ROOT / "src" / "settings" / "SettingsView.tsx"
CSS = ROOT / "src" / "index.css"
PACKAGE = ROOT / "package.json"
CANDIDATE_CONFIG = ROOT / "src-tauri" / "tauri.candidate.conf.json"
BASE_CONFIG = ROOT / "src-tauri" / "tauri.conf.json"
RELEASE = ROOT / "outputs" / "野菜API-桌面客户端-V1-候选版发布清单.md"
BACKEND = ROOT / "outputs" / "野菜API-NewAPI-未来改造清单.md"
ACCOUNT_READINESS = ROOT / "src" / "account" / "readiness.ts"
PREVIEW = ROOT / "src" / "configuration" / "preview.ts"
PREVIEW_VIEW = ROOT / "src" / "configuration" / "ConfigurationPreviewView.tsx"
LOCALE_DIR = ROOT / "src" / "i18n" / "locales"


def text(path: Path) -> str:
    return path.read_text()


def candidate_locale(language: str) -> dict:
    payload = json.loads((LOCALE_DIR / f"{language}.json").read_text())
    return {
        "yeschoyCandidate": payload["yeschoyCandidate"],
        "yeschoyDiagnostics": payload["yeschoyDiagnostics"],
        "yeschoySettings": payload["yeschoySettings"],
    }


def shape(value):
    if isinstance(value, dict):
        return {key: shape(child) for key, child in sorted(value.items())}
    if isinstance(value, list):
        return [shape(child) for child in value]
    return type(value).__name__


def test_beginner_candidate_journey_is_complete_and_truthful() -> None:
    app = text(APP)
    home = text(HOME)
    readiness = text(READINESS)

    assert 'useState<AppView>("home")' in app
    for view in ("home", "account", "setup", "diagnostics", "tools", "settings"):
        assert f'view === "{view}"' in app
        assert f'yeschoyCandidate.nav.{view}' in app
    assert "CandidateHomeView" in app
    for step in ("tools", "setup", "diagnostics", "account"):
        assert f'"{step}"' in home
    assert 'releaseStage: "client_candidate"' in readiness
    assert "productionReady: false" in readiness
    assert "supportedToolCount: 5" in readiness
    assert "supportedLineCount: 2" in readiness
    assert "retainedBackupHistory: false" in readiness
    assert "telemetryUploadEnabled: false" in readiness
    assert "automaticUpdateEnabled: false" in readiness


def test_connectivity_runtime_has_exact_bounded_authority() -> None:
    runtime = text(CONNECTIVITY)
    core = text(CONNECTIVITY_CORE)
    lib = text(TAURI_LIB)
    capability = json.loads(CAPABILITY.read_text())

    assert core.count('host: "yeschoy.com"') == 1
    assert core.count('host: "api.yeschoy.com"') == 1
    assert core.count("port: 443") == 2
    assert '#[serde(rename_all = "camelCase", deny_unknown_fields)]' in runtime
    request_body = runtime.split("pub struct ConnectivityRequest", 1)[1].split("}", 1)[0]
    assert re.findall(r"\b[a-z_]+:\s", request_body) == ["request_id: "]
    assert "lookup_host((line.host, line.port))" in runtime
    assert "TcpStream::connect(address)" in runtime
    assert "tokio::join!" in runtime
    assert "check_line_connectivity_read_only" in lib
    assert "scan_tools_read_only" in lib
    assert capability["permissions"] == ["core:default"]
    forbidden = (
        "reqwest",
        "ureq",
        "hyper",
        "Authorization",
        "Cookie",
        "api_key",
        "request_body",
        "write(",
        "File::create",
        "Command::new",
    )
    assert not [needle for needle in forbidden if needle in runtime]


def test_connectivity_ui_rejects_stale_or_invalid_results() -> None:
    view = text(DIAGNOSTICS)
    contract = text(DIAGNOSTIC_CONTRACT)

    assert 'invoke<ConnectivityResponse>(\n        "check_line_connectivity_read_only"' in view
    assert "latestRequestRef.current = requestId" in view
    assert view.count("latestRequestRef.current !== requestId") >= 2
    assert "isCompleteConnectivityProjection(next, requestId)" in view
    assert "reachableCount === next.lines.length" in view
    assert "reachableCount === 0" in view
    assert 'type DiagnosticsPhase =' in view
    for phase in ("idle", "checking", "success", "partial", "empty", "error"):
        assert f'"{phase}"' in view
    assert "response.lines.length !== CONNECTIVITY_LINES.length" in contract
    assert "candidates.length !== 1" in contract
    assert "response.requestId !== expectedRequestId" in contract
    assert "yeschoyDiagnostics.disclaimerTitle" in view
    assert "yeschoyDiagnostics.disclaimerBody" in view


def test_settings_exposes_security_truth_without_new_authority() -> None:
    settings = text(SETTINGS)
    readiness = text(READINESS)

    for row in ("apiKeys", "configFiles", "backupHistory", "telemetry"):
        assert f'"{row}"' in settings
    for row in ("macos", "windows", "updater"):
        assert f'"{row}"' in settings
    assert 'window.localStorage.setItem("language", language)' in settings
    assert "telemetryUploadEnabled: false" in readiness
    assert "automaticUpdateEnabled: false" in readiness
    forbidden = ("invoke(", "fetch(", "window.open", "document.cookie", "accessToken", "refreshToken")
    assert not [needle for needle in forbidden if needle in settings]


def test_candidate_locales_and_responsive_states_are_complete() -> None:
    locales = [candidate_locale(language) for language in ("zh", "zh-TW", "en", "ja")]
    expected_shape = shape(locales[0])
    assert all(shape(locale) == expected_shape for locale in locales[1:])
    for locale in locales:
        assert all(value for section in locale.values() for value in section)

    css = text(CSS)
    assert "min-width: 320px" in css
    assert "@media (max-width: 560px)" in css
    assert ".view-switcher" in css and "overflow-x: auto" in css
    assert "button:focus-visible" in css
    for workspace in ("candidate-home", "diagnostics-workspace", "settings-workspace"):
        assert f".{workspace}" in css


def test_candidate_packaging_and_release_claims_fail_closed() -> None:
    package = json.loads(PACKAGE.read_text())
    candidate = json.loads(CANDIDATE_CONFIG.read_text())
    base = json.loads(BASE_CONFIG.read_text())
    release = text(RELEASE)

    assert package["scripts"]["build:candidate"] == (
        "pnpm tauri build --config src-tauri/tauri.candidate.conf.json"
    )
    assert candidate["bundle"]["active"] is True
    assert candidate["bundle"]["createUpdaterArtifacts"] is False
    assert base["bundle"]["active"] is False
    assert base["bundle"]["createUpdaterArtifacts"] is False
    for phrase in (
        "不是正式发布批准",
        "Developer ID",
        "hardened runtime",
        "公证",
        "SmartScreen",
        "未签名候选测试包",
        "自动更新",
        "SHA-256",
        "NewAPI",
    ):
        assert phrase in release
    assert "TAURI_SIGNING_PRIVATE_KEY=" not in release


def test_future_newapi_handoff_is_dependency_complete() -> None:
    backend = text(BACKEND)

    for dependency in (
        "/api/desktop/v1/bootstrap",
        "device-authorizations",
        "usage-summary",
        "model-catalog",
        "/api/desktop/v1/pricing",
        "tool-keys",
        "compatibility catalog",
        "telemetry/events",
        "更新私钥不在 NewAPI",
        "RU-005 客户端依赖交接矩阵",
    ):
        assert dependency in backend
    assert "当前未部署、当前不实施" in backend
    assert "不修改 NewAPI、服务器、Nginx、数据库、DNS 或任何线上配置" in backend
    assert "客户端接入顺序" in backend


def test_prior_flows_remain_and_side_effects_stay_blocked() -> None:
    app = text(APP)
    account = text(ACCOUNT_READINESS)
    preview = text(PREVIEW)
    preview_view = text(PREVIEW_VIEW)

    assert app.count('invoke<ScanResponse>("scan_tools_read_only"') == 1
    for tool_id in ("claude", "codex", "opencode", "pi", "dsh"):
        assert f'id: "{tool_id}"' in app
    assert 'ACCOUNT_READINESS_STATUS = "backend_upgrade_required"' in account
    assert 'usdToCny: "6.75"' in account
    assert 'modelId: "" as const' in preview
    assert 'status: "server_catalog_required" as const' in preview
    assert 'endpointStatus: "withheld_unverified"' in preview
    assert 'data-testid="configuration-apply-blocked"' in preview_view
    assert re.search(r'className="blocked-apply"[\s\S]{0,120}?disabled', preview_view)
    for flag in ("networkAttempted", "configurationRead", "configurationWritten", "credentialAccessed"):
        assert f"{flag}: false" in preview
