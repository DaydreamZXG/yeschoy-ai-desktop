from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
BUILD = (ROOT / "src-tauri" / "build.rs").read_text(encoding="utf-8")
ACCOUNT = (ROOT / "src-tauri" / "src" / "account_v2.rs").read_text(
    encoding="utf-8"
)
AUDIT = (ROOT / "outputs" / "野菜API-自动更新与解耦验收.md").read_text(
    encoding="utf-8"
)


def test_happy_path_uses_only_the_selected_allowlisted_page_origin():
    assert '"https://yeschoy.com" | "https://ai.yeschoy.io"' in BUILD
    assert 'const AUTHORIZATION_PAGE_PATH: &str = "/desktop-authorize";' in ACCOUNT
    assert "browser_authorization_url_for_origin" in ACCOUNT
    assert "compiled_authorization_page_origin_drives_the_browser_target" in ACCOUNT
    assert '.append_pair("user_code", user_code)' in ACCOUNT
    assert "https://ai.yeschoy.io/desktop-authorize?user_code=ABCD-2345" in ACCOUNT


def test_failure_rejects_arbitrary_or_ambiguous_origins():
    assert "must be exactly https://yeschoy.com or https://ai.yeschoy.io" in BUILD
    for invariant in [
        'url.scheme() == "https"',
        "url.port().is_none()",
        "url.username().is_empty()",
        "url.password().is_none()",
        'url.path() == "/"',
        "url.query().is_none()",
        "url.fragment().is_none()",
    ]:
        assert invariant in ACCOUNT
    assert '"https://attacker.invalid"' in ACCOUNT
    assert '"https://ai.yeschoy.io.attacker.invalid"' in ACCOUNT


def test_replay_reconstructs_one_canonical_url_without_carrying_remote_query_fields():
    builder_start = ACCOUNT.index("fn browser_authorization_url_for_origin")
    builder_end = ACCOUNT.index("fn browser_authorization_url(", builder_start)
    builder = ACCOUNT[builder_start:builder_end]
    assert "verification_uri_complete" not in builder
    assert "query_pairs_mut" in builder
    assert builder.count('append_pair("user_code", user_code)') == 1
    assert "next" not in builder


def test_stale_or_malicious_server_url_cannot_select_the_browser_origin():
    validation = ACCOUNT.index("let valid = envelope.success")
    local_build = ACCOUNT.index("let Some(browser_url) = browser_authorization_url", validation)
    browser_open = ACCOUNT.index("open_system_browser(&browser_url)", local_build)
    assert validation < local_build < browser_open
    between = ACCOUNT[validation:local_build]
    assert 'authorization.verification_uri == "https://yeschoy.com/desktop-authorize"' in between
    assert "authorization_url_is_allowed(" in between
    assert "authorization.verification_uri_complete" in between


def test_origin_is_compile_time_and_has_no_runtime_mutation_surface():
    assert 'env!("YESCHOY_AUTHORIZATION_PAGE_ORIGIN")' in ACCOUNT
    assert "std::env::var(\"YESCHOY_AUTHORIZATION_PAGE_ORIGIN\")" in BUILD
    assert "cargo:rustc-env=YESCHOY_AUTHORIZATION_PAGE_ORIGIN=" in BUILD
    renderer_sources = "\n".join(
        path.read_text(encoding="utf-8")
        for path in (ROOT / "src").rglob("*")
        if path.suffix in {".ts", ".tsx"}
    )
    assert "YESCHOY_AUTHORIZATION_PAGE_ORIGIN" not in renderer_sources
    assert "不能做成允许用户填写任意网址" in AUDIT


def test_default_build_recovers_to_the_existing_yeschoy_authorization_page():
    assert '.unwrap_or_else(|_| "https://yeschoy.com".to_owned())' in BUILD
    assert 'const CANONICAL_AUTHORIZATION_PAGE_ORIGIN: &str = "https://yeschoy.com";' in ACCOUNT
    unchanged_endpoints = [
        '/api/desktop/v2/device-authorizations',
        '/api/desktop/v2/device-authorizations/token',
        '/api/desktop/v2/sessions/refresh',
        '/api/desktop/v2/sessions/current',
        '/api/user/self',
        '/api/log/self/stat',
        '/api/user/models',
        '/api/pricing',
        '/api/token/',
    ]
    for endpoint in unchanged_endpoints:
        assert endpoint in ACCOUNT
    assert "不激活尚未部署的 PKCE" not in ACCOUNT
