"""RU-063 acceptance: remove only the stale Settings account boundary."""
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]


def source(relative: str) -> str:
    return (ROOT / relative).read_text(encoding="utf-8")


def test_settings_removes_stale_account_unavailable_boundary() -> None:
    settings = source("src/settings/SettingsView.tsx")
    app = source("src/App.tsx")

    assert 'className="settings-boundary"' not in settings
    assert 't("yeschoySettings.backendTitle")' not in settings
    assert 't("yeschoySettings.backendBody")' not in settings
    assert 'setView("account")' in app
    assert 'view === "account"' in app
