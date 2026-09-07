from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
PKCE = (ROOT / "src-tauri" / "src" / "oauth_pkce.rs").read_text(encoding="utf-8")
PRODUCTION = PKCE.split("#[cfg(test)]", 1)[0]
LIB = (ROOT / "src-tauri" / "src" / "lib.rs").read_text(encoding="utf-8")
ACCOUNT = (ROOT / "src-tauri" / "src" / "account_v2.rs").read_text(
    encoding="utf-8"
)
AUDIT = (ROOT / "outputs" / "野菜API-自动更新与解耦验收.md").read_text(
    encoding="utf-8"
)


def _section(source: str, start: str, end: str) -> str:
    begin = source.index(start)
    finish = source.index(end, begin)
    return source[begin:finish]


def test_happy_path_prepares_the_complete_client_owned_pkce_boundary():
    assert "TcpListener::bind((Ipv4Addr::LOCALHOST, 0))" in PRODUCTION
    assert "listener: TcpListener" in PRODUCTION
    assert 'format!("http://127.0.0.1:{port}{CALLBACK_PATH}")' in PRODUCTION
    assert "let state = random_base64url_32()?;" in PRODUCTION
    assert "let mut verifier = random_base64url_32()?;" in PRODUCTION
    assert "Sha256::digest(verifier.as_bytes())" in PRODUCTION
    assert 'append_pair("code_challenge_method", "S256")' in PRODUCTION
    assert 'pub(crate) const DEFAULT_SCOPE: &str = "profile api offline_access";' in PRODUCTION
    assert 'const PARTNER_AUTHORIZATION_ORIGIN: &str = "https://ai.yeschoy.io";' in PRODUCTION
    assert 'pub(crate) const TOKEN_ENDPOINT: &str = "https://yeschoy.com/api/oauth/token";' in PRODUCTION
    assert 'pub(crate) const USERINFO_ENDPOINT: &str = "https://yeschoy.com/api/oauth/userinfo";' in PRODUCTION
    assert 'pub(crate) const REVOKE_ENDPOINT: &str = "https://yeschoy.com/api/oauth/revoke";' in PRODUCTION
    assert "rfc_7636_s256_vector_matches" in PKCE


def test_failure_paths_reject_non_loopback_or_ambiguous_callback_input():
    for invariant in [
        'raw.strip_prefix("http://127.0.0.1:")',
        'url.host_str() == Some("127.0.0.1")',
        'url.path() == CALLBACK_PATH',
        'if method != "GET"',
        "if host != Some(expected_host)",
        "if path != CALLBACK_PATH",
        "if slot.replace(value.into_owned()).is_some()",
        "if code.is_some() == error.is_some()",
        "const MAX_CALLBACK_BYTES: usize = 8 * 1024",
        "const MAX_TOKEN_RESPONSE_BYTES: usize = 2 * 1024 * 1024",
    ]:
        assert invariant in PRODUCTION
    assert "callback_parser_rejects_ambiguous_and_foreign_requests" in PKCE
    assert "#[derive(Deserialize)]" in PRODUCTION
    assert "#[serde(deny_unknown_fields)]" in PRODUCTION
    # Secret-bearing wrappers intentionally do not implement Debug or Clone.
    code_block = _section(PRODUCTION, "pub(crate) struct AuthorizationCode", "pub(crate) struct SensitiveFormRequest")
    token_block = _section(PRODUCTION, "pub(crate) struct OAuthTokenBundle", "pub(crate) struct DeviceMetadata")
    assert "derive(" not in code_block
    assert "derive(" not in token_block


def test_replay_is_claimed_once_and_one_time_forms_have_no_retry_loop():
    assert "AtomicU8" in PRODUCTION
    assert "PHASE_WAITING" in PRODUCTION
    assert "PHASE_CLAIMED" in PRODUCTION
    assert PRODUCTION.count("compare_exchange(") >= 3
    assert "wrong_state_does_not_consume_then_valid_callback_claims_once" in PKCE
    assert "grant_type\", \"authorization_code" in PRODUCTION
    assert "grant_type\", \"refresh_token" in PRODUCTION
    assert "client_secret" not in PRODUCTION
    # This preflight module only constructs bounded requests. Network execution,
    # and therefore automatic replay of a code or refresh token, does not exist here.
    assert "reqwest::Client" not in PRODUCTION
    assert ".post(" not in PRODUCTION
    assert ".send(" not in PRODUCTION


def test_stale_cancelled_and_wrong_state_requests_do_not_become_credentials():
    assert "pub(crate) fn cancel(&self) -> bool" in PRODUCTION
    assert "deadline: Instant" in PRODUCTION
    assert "PHASE_CANCELLED" in PRODUCTION
    assert "PHASE_EXPIRED" in PRODUCTION
    assert "fn expire_if_needed(&self) -> bool" in PRODUCTION
    assert "constant_time_equal(state.as_bytes(), expected_state.as_bytes())" in PRODUCTION
    assert "cancellation_and_expiry_block_claims" in PKCE
    assert "wrong_state_does_not_consume_then_valid_callback_claims_once" in PKCE


def test_race_controls_hold_the_bound_listener_and_atomically_claim_one_callback():
    prepare = _section(
        PRODUCTION,
        "async fn prepare_for_origin",
        "pub(crate) fn browser_url",
    )
    assert prepare.index("TcpListener::bind") < prepare.index("authorization_url(")
    assert "listener," in prepare
    assert "listener: TcpListener" in PRODUCTION
    claim = _section(PRODUCTION, "fn claim_valid_callback", "#[derive(Clone, Copy, Debug")
    assert "compare_exchange(" in claim
    assert "PHASE_WAITING" in claim
    assert "PHASE_CLAIMED" in claim
    assert "Ordering::AcqRel" in claim
    assert "ClaimResult::AlreadyHandled" in claim


def test_recovery_keeps_the_existing_device_flow_active_and_pkce_dormant():
    assert "#[allow(dead_code)]\nmod oauth_pkce;" in LIB
    assert "Deliberately dormant" in LIB
    assert "oauth_pkce::" not in LIB
    assert "/api/desktop/v2/device-authorizations" in ACCOUNT
    assert "/api/desktop/v2/device-authorizations/token" in ACCOUNT
    assert "account_begin_authorization_v2" in ACCOUNT
    for path in (ROOT / "src-tauri" / "src").rglob("*.rs"):
        if path.name in {"lib.rs", "oauth_pkce.rs"}:
            continue
        assert "oauth_pkce" not in path.read_text(encoding="utf-8"), path
    assert "客户端侧已经完成可编译、但默认休眠的 PKCE 预埋" in AUDIT
    assert "不需要等待服务端部署才能完成客户端开发" in AUDIT
