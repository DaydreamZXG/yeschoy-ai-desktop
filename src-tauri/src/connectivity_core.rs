use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LineSpec {
    pub line_id: &'static str,
    pub display_name: &'static str,
    pub root_url: &'static str,
    pub host: &'static str,
    pub port: u16,
}

pub const CONNECTIVITY_LINES: [LineSpec; 2] = [
    LineSpec {
        line_id: "mainland_optimized",
        display_name: "大陆优化",
        root_url: "https://yeschoy.com",
        host: "yeschoy.com",
        port: 443,
    },
    LineSpec {
        line_id: "global_accelerated",
        display_name: "全球加速",
        root_url: "https://api.yeschoy.com",
        host: "api.yeschoy.com",
        port: 443,
    },
];

// PRD 6.7（批次 3 #2）：诊断按层执行，每层通过/失败/跳过三态。
// 顺序即执行顺序：上游失败后，下游层记录为 skipped。
pub const CONNECTIVITY_LAYER_IDS: [&str; 4] = ["dns", "tcp", "tls", "api_key"];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerReasonCode {
    DnsResolved,
    DnsResolutionFailed,
    DnsLookupTimedOut,
    Tcp443Reachable,
    TcpConnectionFailed,
    TcpConnectTimedOut,
    TlsHandshakeVerified,
    TlsCertificateInvalid,
    TlsHandshakeFailed,
    TlsHandshakeTimedOut,
    SessionTokenValid,
    SessionTokenRejected,
    ApiProbeError,
    SkippedUpstreamFailed,
    SkippedNoSavedSession,
    SessionProbeUnavailable,
}

impl LayerReasonCode {
    pub const fn layer(self) -> &'static str {
        match self {
            LayerReasonCode::DnsResolved
            | LayerReasonCode::DnsResolutionFailed
            | LayerReasonCode::DnsLookupTimedOut => "dns",
            LayerReasonCode::Tcp443Reachable
            | LayerReasonCode::TcpConnectionFailed
            | LayerReasonCode::TcpConnectTimedOut => "tcp",
            LayerReasonCode::TlsHandshakeVerified
            | LayerReasonCode::TlsCertificateInvalid
            | LayerReasonCode::TlsHandshakeFailed
            | LayerReasonCode::TlsHandshakeTimedOut => "tls",
            LayerReasonCode::SessionTokenValid
            | LayerReasonCode::SessionTokenRejected
            | LayerReasonCode::ApiProbeError
            | LayerReasonCode::SkippedNoSavedSession
            | LayerReasonCode::SessionProbeUnavailable => "api_key",
            LayerReasonCode::SkippedUpstreamFailed => "skipped",
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectivityLayerResult {
    pub layer: &'static str,
    pub status: LayerStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    pub reason_code: LayerReasonCode,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectivityLineResult {
    pub line_id: &'static str,
    pub display_name: &'static str,
    pub root_url: &'static str,
    pub host: &'static str,
    pub port: u16,
    pub layers: Vec<ConnectivityLayerResult>,
}

pub fn request_id_is_valid(request_id: &str) -> bool {
    !request_id.is_empty()
        && request_id.len() <= 64
        && request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::{request_id_is_valid, CONNECTIVITY_LAYER_IDS, CONNECTIVITY_LINES};

    #[test]
    fn diagnostics_contract_catalog_is_exact_and_fixed_to_tls_port() {
        assert_eq!(CONNECTIVITY_LINES.len(), 2);
        assert_eq!(CONNECTIVITY_LINES[0].host, "yeschoy.com");
        assert_eq!(CONNECTIVITY_LINES[1].host, "api.yeschoy.com");
        assert!(CONNECTIVITY_LINES
            .iter()
            .all(|line| line.host != "yeschoy.pro"));
        assert!(CONNECTIVITY_LINES.iter().all(|line| line.port == 443));
    }

    #[test]
    fn diagnostics_contract_layer_catalog_is_fixed_and_ordered() {
        assert_eq!(CONNECTIVITY_LAYER_IDS, ["dns", "tcp", "tls", "api_key"]);
    }

    #[test]
    fn diagnostics_contract_request_id_rejects_network_or_path_input() {
        assert!(request_id_is_valid("line-abc_123"));
        assert!(!request_id_is_valid("https://example.com"));
        assert!(!request_id_is_valid("../secret"));
        assert!(!request_id_is_valid(""));
        assert!(request_id_is_valid(&"a".repeat(64)));
        assert!(!request_id_is_valid(&"a".repeat(65)));
    }
}
