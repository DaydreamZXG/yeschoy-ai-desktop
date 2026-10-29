//! Existing New API account protocol over the hardened native account client.
//! No independent client, redirect policy, filesystem, or credential store.

use reqwest::{Method, Url};
use serde_json::Value;

use crate::application::ports::station::{StationMethod, StationProvider, StationProviderFailure};
use crate::domain::station::StationOrigin;

pub(crate) struct NewApiStationProvider<'a> {
    origin: StationOrigin,
    access_token: &'a str,
}

impl NewApiStationProvider<'_> {
    /// The canonical station identity this provider is bound to. The account
    /// token stays private to the provider. Reserved for callers that must prove
    /// same-origin before acting; activation currently proves it by construction.
    #[allow(dead_code)]
    pub(crate) fn origin(&self) -> &StationOrigin {
        &self.origin
    }
}

impl<'a> NewApiStationProvider<'a> {
    /// Identity validation only. The caller supplies the already authorized
    /// native session token and retains responsibility for session-epoch checks.
    pub(crate) fn new(origin: &str, access_token: &'a str) -> Result<Self, StationProviderFailure> {
        Ok(Self {
            origin: StationOrigin::parse(origin)
                .map_err(|_| StationProviderFailure::InvalidOrigin)?,
            access_token,
        })
    }

    fn account_url(&self, path: &str) -> Result<String, StationProviderFailure> {
        let route = path.split('?').next().unwrap_or_default();
        if !route.starts_with("/api/")
            || path.contains('#')
            || path.contains('\\')
            || path.chars().any(|c| c.is_control() || c.is_whitespace())
            // Current account routes are fixed ASCII paths plus numeric IDs;
            // percent encoding belongs only in the query, never in path segments
            // that a server might decode into traversal or another endpoint.
            || route.contains('%')
            || route.split('/').any(|segment| matches!(segment, "." | ".."))
        {
            return Err(StationProviderFailure::InvalidPath);
        }
        let raw = format!("{}{path}", self.origin.as_str());
        let url = Url::parse(&raw).map_err(|_| StationProviderFailure::InvalidPath)?;
        // The URL parser must not normalize the declared request into another
        // route or authority (notably dot segments and backslashes).
        if url.as_str() != raw || url.origin().ascii_serialization() != self.origin.as_str() {
            return Err(StationProviderFailure::InvalidPath);
        }
        Ok(raw)
    }

    async fn account_json_using(
        &self,
        transport: &impl AccountTransport,
        method: StationMethod,
        path: &str,
        body: Option<Value>,
    ) -> Result<(u16, Value), StationProviderFailure> {
        let url = self.account_url(path)?;
        let method = match method {
            StationMethod::Get => Method::GET,
            StationMethod::Post => Method::POST,
            StationMethod::Delete => Method::DELETE,
        };
        transport
            .request(method, &url, self.access_token, body)
            .await
            .map_err(|_| StationProviderFailure::Transport)
    }
}

impl StationProvider for NewApiStationProvider<'_> {
    async fn account_json(
        &self,
        method: StationMethod,
        path: &str,
        body: Option<Value>,
    ) -> Result<(u16, Value), StationProviderFailure> {
        self.account_json_using(&NativeAccountTransport, method, path, body)
            .await
    }
}

// Private seam for transport contract tests, not an application HTTP dependency.
trait AccountTransport: Sync {
    fn request(
        &self,
        method: Method,
        url: &str,
        access_token: &str,
        body: Option<Value>,
    ) -> impl std::future::Future<Output = Result<(u16, Value), ()>> + Send;
}

struct NativeAccountTransport;
impl AccountTransport for NativeAccountTransport {
    async fn request(
        &self,
        method: Method,
        url: &str,
        access_token: &str,
        body: Option<Value>,
    ) -> Result<(u16, Value), ()> {
        crate::account_v2::native_account_json(method, url, access_token, body).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_api_account_routes_are_origin_bound_and_preserve_protocol_paths() {
        for origin in ["https://third-party.example", "https://api.yeschoy.com"] {
            let provider = NewApiStationProvider::new(origin, "synthetic-session-token").unwrap();
            for route in [
                "/api/user/models",
                "/api/pricing",
                "/api/token/?p=1&size=100",
                "/api/token/",
                "/api/token/42/key",
                "/api/token/42",
            ] {
                assert_eq!(
                    provider.account_url(route).unwrap(),
                    format!("{origin}{route}")
                );
            }
        }
        assert!(NewApiStationProvider::new("https://user@relay.example", "synthetic").is_err());
    }

    #[test]
    fn new_api_rejects_url_authorities_fragments_and_traversal_without_network() {
        let provider = NewApiStationProvider::new("https://relay.example", "synthetic").unwrap();
        for route in [
            "https://evil.example/api/token/",
            "//evil.example/api/token/",
            "/v1/chat/completions",
            "/api/token/#fragment",
            "/api/../token/",
            "/api/./token/",
            "/api/%2e%2e/token/",
            "/api/.%2E/token/",
            "/api/%2f../token/",
            "/api/%252e%252e/token/",
            "/api/token/\\evil",
            "/api/token/?p=1\n",
        ] {
            assert_eq!(
                provider.account_url(route),
                Err(StationProviderFailure::InvalidPath),
                "{route:?}"
            );
        }
    }

    type FixtureCall = (Method, String, String, Option<Value>);

    struct FixtureTransport {
        calls: std::sync::Mutex<Vec<FixtureCall>>,
        fail: bool,
    }
    impl AccountTransport for FixtureTransport {
        async fn request(
            &self,
            method: Method,
            url: &str,
            access_token: &str,
            body: Option<Value>,
        ) -> Result<(u16, Value), ()> {
            self.calls
                .lock()
                .unwrap()
                .push((method, url.into(), access_token.into(), body));
            if self.fail {
                Err(())
            } else {
                Ok((
                    403,
                    serde_json::json!({"success": false, "code": "synthetic_denied"}),
                ))
            }
        }
    }

    #[tokio::test]
    async fn new_api_adapter_preserves_methods_body_status_and_transport_failures() {
        let transport = FixtureTransport {
            calls: Default::default(),
            fail: false,
        };
        let provider =
            NewApiStationProvider::new("https://relay.example", "synthetic-session-token").unwrap();
        let body = serde_json::json!({"name":"synthetic-token-name", "group":"fixture-group"});
        for (method, route, expected) in [
            (StationMethod::Get, "/api/token/?p=1&size=100", Method::GET),
            (StationMethod::Post, "/api/token/", Method::POST),
            (StationMethod::Delete, "/api/token/42", Method::DELETE),
        ] {
            let response = provider
                .account_json_using(&transport, method, route, Some(body.clone()))
                .await
                .unwrap();
            assert_eq!(
                response,
                (
                    403,
                    serde_json::json!({"success":false,"code":"synthetic_denied"})
                )
            );
            assert_eq!(
                transport.calls.lock().unwrap().last().unwrap(),
                &(
                    expected,
                    format!("https://relay.example{route}"),
                    "synthetic-session-token".into(),
                    Some(body.clone())
                )
            );
        }
        let before = transport.calls.lock().unwrap().len();
        assert!(provider
            .account_json_using(&transport, StationMethod::Get, "//evil.example", None)
            .await
            .is_err());
        assert_eq!(transport.calls.lock().unwrap().len(), before);
        let failing = FixtureTransport {
            calls: Default::default(),
            fail: true,
        };
        assert_eq!(
            provider
                .account_json_using(&failing, StationMethod::Post, "/api/token/42/key", None)
                .await,
            Err(StationProviderFailure::Transport)
        );
    }
}
