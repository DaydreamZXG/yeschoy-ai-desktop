from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]


def source(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def test_windows_release_is_self_contained_on_a_clean_machine() -> None:
    cargo_config = source(".cargo/config.toml")
    assert "[target.x86_64-pc-windows-msvc]" in cargo_config
    assert 'rustflags = ["-C", "target-feature=+crt-static"]' in cargo_config


def test_silent_upgrade_restarts_the_app_in_a_visible_state() -> None:
    installer = source("src-tauri/windows/installer.nsi")
    success = installer.split("Function .onInstSuccess", 1)[1].split(
        "FunctionEnd", 1
    )[0]
    assert 'ExecShell "open" "$INSTDIR\\野菜API.exe" "" SW_SHOWNORMAL' in success
    assert "Exec '\"$INSTDIR\\野菜API.exe\"'" not in success
    assert success.index("ClearErrors") < success.index("ExecShell") < success.index(
        "IfErrors"
    )
