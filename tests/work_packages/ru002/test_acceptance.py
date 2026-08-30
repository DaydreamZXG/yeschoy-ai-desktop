import json
from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[3]
READINESS = ROOT / "src" / "account" / "readiness.ts"
APP = ROOT / "src" / "App.tsx"
ZH = ROOT / "src" / "i18n" / "locales" / "zh.json"
SPEC = ROOT / "outputs" / "野菜API-NewAPI-未来改造清单.md"


def account_copy() -> dict:
    return json.loads(ZH.read_text())["yeschoyAccount"]


def test_readiness_projection_is_fail_closed() -> None:
    source = READINESS.read_text()

    assert source.count('"backend_upgrade_required"') == 1
    assert 'networkAttempted: false' in source
    assert 'serverNamespace: "/api/desktop/v1"' in source
    for capability in (
        "authorization",
        "accountSummary",
        "usage",
        "pricing",
        "recharge",
    ):
        assert re.search(
            rf"{capability}: typeof REMOTE_CAPABILITY_STATUS", source
        )
        assert re.search(rf"{capability}: REMOTE_CAPABILITY_STATUS", source)
    assert 'REMOTE_CAPABILITY_STATUS = "unavailable"' in source


def test_account_shell_never_fabricates_remote_values() -> None:
    app = APP.read_text()
    copy = account_copy()

    assert 'useState<AppView>("account")' in app
    assert copy["loginToView"] == "登录后显示"
    assert copy["noRemoteData"] == "当前没有远端账户数据"
    assert set(copy["metrics"]) == {"balance", "todayUsage", "monthUsage"}
    for label in (
        "modelId",
        "officialPrice",
        "actualPrice",
        "rechargeTitle",
    ):
        assert copy[label]
    assert 'className="disabled-action"' in app
    assert not re.search(r"[¥￥$]\s*\d", app)
    assert not re.search(r"\b(?:gpt|claude|glm|deepseek)-\d", app, re.I)


def test_account_shell_adds_no_network_or_secret_authority() -> None:
    account_source = "\n".join(
        path.read_text() for path in sorted((ROOT / "src" / "account").rglob("*.ts"))
    )
    forbidden = (
        "fetch(",
        "invoke(",
        "http://",
        "https://",
        "localStorage",
        "sessionStorage",
        "document.cookie",
        "window.open",
        "accessToken",
        "refreshToken",
        "apiKey",
    )
    assert not [item for item in forbidden if item in account_source]
    assert account_source.count("networkAttempted: false") == 2


def test_fixed_fx_is_display_only() -> None:
    readiness = READINESS.read_text()
    app = APP.read_text()
    copy = account_copy()

    assert 'usdToCny: "6.75"' in readiness
    assert 'purpose: "display_only"' in readiness
    assert 'liveRate: false' in readiness
    assert "1 USD = {accountReadiness.comparisonFx.usdToCny} CNY" in app
    assert copy["fxDisclaimer"] == "只用于价格对比，不是实时汇率，也不参与扣费"
    assert not re.search(r"(?:official|actual).*\d+\.\d+", app, re.I)


def test_ru001_discovery_remains_wired() -> None:
    app = APP.read_text()

    assert app.count('invoke<ScanResponse>("scan_tools_read_only"') == 1
    assert "latestRequestRef.current = requestId" in app
    assert app.count("latestRequestRef.current !== requestId") >= 2
    for tool_id in ("claude", "codex", "opencode", "pi", "dsh"):
        assert f'id: "{tool_id}"' in app
    assert "configure" not in app.casefold()
    assert "api key" not in app.casefold()


def test_future_newapi_spec_is_current_and_proposed() -> None:
    spec = SPEC.read_text()

    assert "918427d8ab41f6adaa4113d0496f1f8621855b70" in spec
    assert "v1.0.0-rc.27" in spec
    assert "HTTP 404" in spec
    assert "HTTP 401" in spec
    assert "37 个模型" in spec
    assert "当前未部署" in spec
    assert "未来接口提案" in spec
    assert "当前 RU-002 只更新客户端界面和这份建议" in spec
    assert "不修改 NewAPI、服务器、Nginx、数据库、DNS 或任何线上配置" in spec
