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
        root_url: "https://yeschoy.pro",
        host: "yeschoy.pro",
        port: 443,
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectivityStatus {
    Reachable,
    DnsFailed,
    ConnectFailed,
    TimedOut,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectivityReasonCode {
    Tcp443Reachable,
    DnsResolutionFailed,
    TcpConnectionFailed,
    ConnectivityCheckTimedOut,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectivityLineResult {
    pub line_id: &'static str,
    pub display_name: &'static str,
    pub root_url: &'static str,
    pub host: &'static str,
    pub port: u16,
    pub status: ConnectivityStatus,
    pub latency_ms: u64,
    pub reason_code: ConnectivityReasonCode,
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
    use super::{request_id_is_valid, CONNECTIVITY_LINES};

    #[test]
    fn catalog_is_exact_and_fixed_to_tls_port() {
        assert_eq!(CONNECTIVITY_LINES.len(), 2);
        assert_eq!(CONNECTIVITY_LINES[0].host, "yeschoy.com");
        assert_eq!(CONNECTIVITY_LINES[1].host, "yeschoy.pro");
        assert!(CONNECTIVITY_LINES
            .iter()
            .all(|line| line.host != "api.yeschoy.com"));
        assert!(CONNECTIVITY_LINES.iter().all(|line| line.port == 443));
    }

    #[test]
    fn request_id_rejects_network_or_path_input() {
        assert!(request_id_is_valid("line-abc_123"));
        assert!(!request_id_is_valid("https://example.com"));
        assert!(!request_id_is_valid("../secret"));
        assert!(!request_id_is_valid(""));
    }
}
