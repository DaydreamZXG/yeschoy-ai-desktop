from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]


def read(relative: str) -> str:
    return (ROOT / relative).read_text(encoding="utf-8")


def test_connection_ux_lifecycle_matrix():
    preview = read("src/configuration/ConfigurationPreviewView.tsx")
    controller = read("src/configuration/connections.tsx")
    launcher = read("src/configuration/launchApi.ts")
    activation = read("src-tauri/src/tool_activation.rs")
    codex_adapter = read("src-tauri/src/tool_adapters/codex_desktop.rs")
    audit = read("outputs/野菜API-接入体验审计.md")

    # Consent is dismissed before the asynchronous graceful-restart request,
    # so the Radix modal cannot cover the whole client during configuration.
    dismiss = preview.index(
        "setRestartPromptContext(handoff.nextPromptContext);",
        preview.index("const handoff = runningAppHandoff("),
    )
    restart = preview.index("void apply(undefined, true);", dismiss)
    assert dismiss < restart
    assert "只剩后台进程" in preview
    assert "不会按名称结束其他程序" in preview
    assert "你不需要手动退出" in preview

    # Launch-only state must not disable the independent update action.
    apply_button = preview[preview.index('data-testid="configuration-apply-action"') - 700 :]
    apply_button = apply_button[: apply_button.index("</button>")]
    assert "connections?.opening" not in apply_button
    assert "更新接入设置" in preview
    assert "connection-action-deck" in preview

    # Focus refresh retains usable state and a lost launch reply is bounded.
    refresh = controller[controller.index("const refresh = useCallback") : controller.index("const restore = useCallback")]
    assert "initialInspectionSettled.current" in refresh
    assert "if (!initialInspectionSettled.current) setLoading(true)" in refresh
    assert "openingOperation" in controller
    assert "OPEN_REQUEST_DEADLINE_MS" in launcher
    assert 'Error("open_request_timed_out")' in launcher

    # A desktop app may flush config as it exits. The rollback snapshot must be
    # captured after that graceful exit, or the app's own write is misreported
    # as a higher-precedence override.
    configure = activation[activation.index("let default_transport") :]
    final_quit = configure.index("desktop_lifecycle::quit_for_reconfigure")
    fresh_snapshot = configure.index("prepare_adapter(", final_quit)
    assert final_quit < fresh_snapshot
    assert "Snapshot only" in configure[final_quit:fresh_snapshot]

    # Existing CC Switch/older-client catalogs are recoverable state, not a
    # reason to strand beginners. CODEX_HOME must select the directory Codex
    # itself reads, while unrelated external catalog files remain untouched.
    assert 'std::env::var_os("CODEX_HOME")' in codex_adapter
    assert "user_owned_catalog_is_temporarily_shadowed" in codex_adapter
    assert "external_catalog_path" in codex_adapter

    # The product copy and audit cover all supported surface classes.
    assert '"graceful_desktop_restart"' in preview
    assert '"new_terminal_session"' in preview
    assert '"browser_launch"' in preview
    for phrase in [
        "Codex Desktop / Claude Desktop",
        "Claude Code / Pi / Hermes / OpenClaw",
        "DSH web",
        "不打包",
    ]:
        assert phrase in audit
