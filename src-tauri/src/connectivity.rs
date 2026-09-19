use std::{
    io,
    net::SocketAddr,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tokio::{
    net::{lookup_host, TcpStream},
    time::timeout,
};

use crate::account_v2::{self, AccountV2State, ProbeSessionFailure};
use crate::connectivity_core::{
    request_id_is_valid, ConnectivityLayerResult, ConnectivityLineResult, LayerReasonCode,
    LayerStatus, LineSpec, CONNECTIVITY_LAYER_IDS, CONNECTIVITY_LINES,
};

const DNS_TIMEOUT: Duration = Duration::from_secs(3);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const TLS_TIMEOUT: Duration = Duration::from_secs(10);
const API_PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const STATUS_PROBE_PATH: &str = "/api/status";
const MODELS_PROBE_PATH: &str = "/api/user/models";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectivityRequest {
    request_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectivityResponse {
    request_id: String,
    schema_version: u8,
    started_at_epoch_ms: u64,
    completed_at_epoch_ms: u64,
    lines: Vec<ConnectivityLineResult>,
}

fn unix_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn bounded_ms(elapsed: Duration) -> u64 {
    elapsed.as_millis().min(60_000) as u64
}

// 诊断 Key 有效性层的会话输入（PRD 6.7 层⑤，批次 3 #2）。
enum SessionProbe {
    Ready(String),
    SignedOut,
    NoSession,
    Unavailable,
}

async fn connect_any(addresses: &[SocketAddr]) -> io::Result<()> {
    let mut last_error = None;
    for address in addresses {
        match TcpStream::connect(address).await {
            Ok(stream) => {
                drop(stream);
                return Ok(());
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no address")))
}

fn error_chain_mentions_certificate(error: &reqwest::Error) -> bool {
    let mut current: Option<&dyn std::error::Error> = Some(error);
    while let Some(error) = current {
        let text = error.to_string().to_ascii_lowercase();
        if text.contains("certificate") || text.contains("cert") || text.contains("ssl") {
            return true;
        }
        current = error.source();
    }
    false
}

enum TlsProbeOutcome {
    Verified(u64),
    CertificateInvalid(u64),
    TimedOut(u64),
    Failed(u64),
}

// TLS 握手层（PRD 6.7 层③）：任何 HTTP 响应都证明证书链与有效期校验通过
// （reqwest/native-tls 在握手阶段完成校验）；失败再按证书/超时/握手细分。
async fn tls_probe(line: LineSpec) -> TlsProbeOutcome {
    let started = Instant::now();
    let url = format!("{}{}", line.root_url, STATUS_PROBE_PATH);
    let Ok(client) = account_v2::shared_http_client() else {
        return TlsProbeOutcome::Failed(bounded_ms(started.elapsed()));
    };
    let request = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/json")
        .build();
    let Ok(request) = request else {
        return TlsProbeOutcome::Failed(bounded_ms(started.elapsed()));
    };
    match timeout(TLS_TIMEOUT, client.execute(request)).await {
        Err(_) => TlsProbeOutcome::TimedOut(bounded_ms(started.elapsed())),
        Ok(Err(error)) => {
            let elapsed = bounded_ms(started.elapsed());
            if error.is_timeout() {
                TlsProbeOutcome::TimedOut(elapsed)
            } else if error_chain_mentions_certificate(&error) {
                TlsProbeOutcome::CertificateInvalid(elapsed)
            } else {
                TlsProbeOutcome::Failed(elapsed)
            }
        }
        Ok(Ok(_)) => TlsProbeOutcome::Verified(bounded_ms(started.elapsed())),
    }
}

// API Key 有效性层（PRD 6.7 层⑤）：GET /api/user/models，401/403 视为失效；
// 该请求只读模型清单，不消费额度。
async fn api_key_probe(
    line: LineSpec,
    access_token: &str,
) -> (LayerStatus, Option<u64>, LayerReasonCode) {
    let started = Instant::now();
    let url = format!("{}{}", line.root_url, MODELS_PROBE_PATH);
    let Ok(client) = account_v2::shared_http_client() else {
        return (LayerStatus::Failed, None, LayerReasonCode::ApiProbeError);
    };
    let request = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {access_token}"),
        )
        .build();
    let Ok(request) = request else {
        return (LayerStatus::Failed, None, LayerReasonCode::ApiProbeError);
    };
    match timeout(API_PROBE_TIMEOUT, client.execute(request)).await {
        Err(_) => (
            LayerStatus::Failed,
            Some(bounded_ms(started.elapsed())),
            LayerReasonCode::ApiProbeError,
        ),
        Ok(Err(_)) => (
            LayerStatus::Failed,
            Some(bounded_ms(started.elapsed())),
            LayerReasonCode::ApiProbeError,
        ),
        Ok(Ok(response)) => {
            let elapsed = bounded_ms(started.elapsed());
            let status = response.status();
            if status == reqwest::StatusCode::UNAUTHORIZED
                || status == reqwest::StatusCode::FORBIDDEN
            {
                (
                    LayerStatus::Failed,
                    Some(elapsed),
                    LayerReasonCode::SessionTokenRejected,
                )
            } else if status.is_success() {
                (
                    LayerStatus::Passed,
                    Some(elapsed),
                    LayerReasonCode::SessionTokenValid,
                )
            } else {
                (
                    LayerStatus::Failed,
                    Some(elapsed),
                    LayerReasonCode::ApiProbeError,
                )
            }
        }
    }
}

async fn check_line(line: LineSpec, session: &SessionProbe) -> ConnectivityLineResult {
    let mut layers: Vec<ConnectivityLayerResult> = Vec::with_capacity(CONNECTIVITY_LAYER_IDS.len());

    // 层① DNS
    let dns_started = Instant::now();
    let addresses = match timeout(DNS_TIMEOUT, lookup_host((line.host, line.port))).await {
        Err(_) => {
            layers.push(ConnectivityLayerResult {
                layer: "dns",
                status: LayerStatus::Failed,
                latency_ms: Some(bounded_ms(dns_started.elapsed())),
                reason_code: LayerReasonCode::DnsLookupTimedOut,
            });
            Vec::new()
        }
        Ok(Err(_)) => {
            layers.push(ConnectivityLayerResult {
                layer: "dns",
                status: LayerStatus::Failed,
                latency_ms: Some(bounded_ms(dns_started.elapsed())),
                reason_code: LayerReasonCode::DnsResolutionFailed,
            });
            Vec::new()
        }
        Ok(Ok(resolved)) => resolved.collect::<Vec<_>>(),
    };
    let dns_ok = !addresses.is_empty();
    if dns_ok {
        layers.push(ConnectivityLayerResult {
            layer: "dns",
            status: LayerStatus::Passed,
            latency_ms: Some(bounded_ms(dns_started.elapsed())),
            reason_code: LayerReasonCode::DnsResolved,
        });
    } else if matches!(layers.last(), Some(result) if result.reason_code == LayerReasonCode::DnsResolved)
    {
        // 不可达分支：空地址列表时上面的失败分支已记录。
    } else if layers.last().is_none() {
        layers.push(ConnectivityLayerResult {
            layer: "dns",
            status: LayerStatus::Failed,
            latency_ms: Some(bounded_ms(dns_started.elapsed())),
            reason_code: LayerReasonCode::DnsResolutionFailed,
        });
    }

    // 层④ TCP（上游失败即跳过，层号沿用 PRD 6.7 的层级编号）
    let mut tcp_ok = false;
    if !dns_ok {
        layers.push(ConnectivityLayerResult {
            layer: "tcp",
            status: LayerStatus::Skipped,
            latency_ms: None,
            reason_code: LayerReasonCode::SkippedUpstreamFailed,
        });
    } else {
        let tcp_started = Instant::now();
        match timeout(CONNECT_TIMEOUT, connect_any(&addresses)).await {
            Err(_) => layers.push(ConnectivityLayerResult {
                layer: "tcp",
                status: LayerStatus::Failed,
                latency_ms: Some(bounded_ms(tcp_started.elapsed())),
                reason_code: LayerReasonCode::TcpConnectTimedOut,
            }),
            Ok(Err(_)) => layers.push(ConnectivityLayerResult {
                layer: "tcp",
                status: LayerStatus::Failed,
                latency_ms: Some(bounded_ms(tcp_started.elapsed())),
                reason_code: LayerReasonCode::TcpConnectionFailed,
            }),
            Ok(Ok(())) => {
                tcp_ok = true;
                layers.push(ConnectivityLayerResult {
                    layer: "tcp",
                    status: LayerStatus::Passed,
                    latency_ms: Some(bounded_ms(tcp_started.elapsed())),
                    reason_code: LayerReasonCode::Tcp443Reachable,
                });
            }
        }
    }

    // 层③ TLS 握手（证书链/有效期）
    let mut tls_ok = false;
    if !tcp_ok {
        layers.push(ConnectivityLayerResult {
            layer: "tls",
            status: LayerStatus::Skipped,
            latency_ms: None,
            reason_code: LayerReasonCode::SkippedUpstreamFailed,
        });
    } else {
        match tls_probe(line).await {
            TlsProbeOutcome::Verified(ms) => {
                tls_ok = true;
                layers.push(ConnectivityLayerResult {
                    layer: "tls",
                    status: LayerStatus::Passed,
                    latency_ms: Some(ms),
                    reason_code: LayerReasonCode::TlsHandshakeVerified,
                });
            }
            TlsProbeOutcome::CertificateInvalid(ms) => layers.push(ConnectivityLayerResult {
                layer: "tls",
                status: LayerStatus::Failed,
                latency_ms: Some(ms),
                reason_code: LayerReasonCode::TlsCertificateInvalid,
            }),
            TlsProbeOutcome::TimedOut(ms) => layers.push(ConnectivityLayerResult {
                layer: "tls",
                status: LayerStatus::Failed,
                latency_ms: Some(ms),
                reason_code: LayerReasonCode::TlsHandshakeTimedOut,
            }),
            TlsProbeOutcome::Failed(ms) => layers.push(ConnectivityLayerResult {
                layer: "tls",
                status: LayerStatus::Failed,
                latency_ms: Some(ms),
                reason_code: LayerReasonCode::TlsHandshakeFailed,
            }),
        }
    }

    // 层⑤ API Key 有效性（无会话即跳过；会话被拒即失败）
    if !tls_ok {
        layers.push(ConnectivityLayerResult {
            layer: "api_key",
            status: LayerStatus::Skipped,
            latency_ms: None,
            reason_code: LayerReasonCode::SkippedUpstreamFailed,
        });
    } else {
        match session {
            SessionProbe::Ready(access_token) => {
                let (status, latency_ms, reason_code) = api_key_probe(line, access_token).await;
                layers.push(ConnectivityLayerResult {
                    layer: "api_key",
                    status,
                    latency_ms,
                    reason_code,
                });
            }
            SessionProbe::NoSession => layers.push(ConnectivityLayerResult {
                layer: "api_key",
                status: LayerStatus::Skipped,
                latency_ms: None,
                reason_code: LayerReasonCode::SkippedNoSavedSession,
            }),
            SessionProbe::SignedOut => layers.push(ConnectivityLayerResult {
                layer: "api_key",
                status: LayerStatus::Failed,
                latency_ms: None,
                reason_code: LayerReasonCode::SessionTokenRejected,
            }),
            SessionProbe::Unavailable => layers.push(ConnectivityLayerResult {
                layer: "api_key",
                status: LayerStatus::Skipped,
                latency_ms: None,
                reason_code: LayerReasonCode::SessionProbeUnavailable,
            }),
        }
    }

    ConnectivityLineResult {
        line_id: line.line_id,
        display_name: line.display_name,
        root_url: line.root_url,
        host: line.host,
        port: line.port,
        layers,
    }
}

#[tauri::command]
pub async fn check_line_connectivity_read_only(
    state: tauri::State<'_, AccountV2State>,
    request: ConnectivityRequest,
) -> Result<ConnectivityResponse, String> {
    if !request_id_is_valid(&request.request_id) {
        return Err("invalid_request_id".to_string());
    }
    // 先判断是否存有会话：刷新被拒会删除存储，之后无法区分
    // 「从未登录（跳过）」和「会话失效（失败）」。
    let had_session = account_v2::has_stored_session();
    let session = match account_v2::native_probe_access(&state).await {
        Ok(access_token) => SessionProbe::Ready(access_token),
        Err(ProbeSessionFailure::SignedOut) => {
            if had_session {
                SessionProbe::SignedOut
            } else {
                SessionProbe::NoSession
            }
        }
        Err(ProbeSessionFailure::SecureStorage) | Err(ProbeSessionFailure::ServerUnavailable) => {
            SessionProbe::Unavailable
        }
    };

    let started_at_epoch_ms = unix_epoch_ms();
    let (mainland, global) = tokio::join!(
        check_line(CONNECTIVITY_LINES[0], &session),
        check_line(CONNECTIVITY_LINES[1], &session)
    );

    Ok(ConnectivityResponse {
        request_id: request.request_id,
        schema_version: 2,
        started_at_epoch_ms,
        completed_at_epoch_ms: unix_epoch_ms(),
        lines: vec![mainland, global],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(
        layer: &'static str,
        status: LayerStatus,
        latency_ms: Option<u64>,
        reason_code: LayerReasonCode,
    ) -> ConnectivityLayerResult {
        ConnectivityLayerResult {
            layer,
            status,
            latency_ms,
            reason_code,
        }
    }

    fn line_result(line: LineSpec, layers: Vec<ConnectivityLayerResult>) -> ConnectivityLineResult {
        ConnectivityLineResult {
            line_id: line.line_id,
            display_name: line.display_name,
            root_url: line.root_url,
            host: line.host,
            port: line.port,
            layers,
        }
    }

    fn fixture_response(
        request_id: &str,
        lines: Vec<ConnectivityLineResult>,
    ) -> ConnectivityResponse {
        ConnectivityResponse {
            request_id: request_id.to_owned(),
            schema_version: 2,
            started_at_epoch_ms: 1_788_607_800_000,
            completed_at_epoch_ms: 1_788_607_800_030,
            lines,
        }
    }

    #[test]
    fn diagnostics_contract_native_fixtures_match_real_serialization() {
        // The renderer consumes this same file. The expected wire spelling is
        // never re-created in TypeScript or in a second Rust projection type.
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/work_packages/ru041/fixtures/connectivity-native.json"
        ))
        .expect("shared native connectivity fixture is valid JSON");

        let all_passed = |offset: u64| {
            vec![
                layer(
                    "dns",
                    LayerStatus::Passed,
                    Some(offset + 1),
                    LayerReasonCode::DnsResolved,
                ),
                layer(
                    "tcp",
                    LayerStatus::Passed,
                    Some(offset + 2),
                    LayerReasonCode::Tcp443Reachable,
                ),
                layer(
                    "tls",
                    LayerStatus::Passed,
                    Some(offset + 3),
                    LayerReasonCode::TlsHandshakeVerified,
                ),
                layer(
                    "api_key",
                    LayerStatus::Passed,
                    Some(offset + 4),
                    LayerReasonCode::SessionTokenValid,
                ),
            ]
        };
        let skipped_after_dns_failure = || {
            vec![
                layer(
                    "dns",
                    LayerStatus::Failed,
                    Some(11),
                    LayerReasonCode::DnsResolutionFailed,
                ),
                layer(
                    "tcp",
                    LayerStatus::Skipped,
                    None,
                    LayerReasonCode::SkippedUpstreamFailed,
                ),
                layer(
                    "tls",
                    LayerStatus::Skipped,
                    None,
                    LayerReasonCode::SkippedUpstreamFailed,
                ),
                layer(
                    "api_key",
                    LayerStatus::Skipped,
                    None,
                    LayerReasonCode::SkippedUpstreamFailed,
                ),
            ]
        };

        let reachable = fixture_response(
            "diag-fixture-reachable",
            vec![
                line_result(CONNECTIVITY_LINES[0], all_passed(10)),
                line_result(CONNECTIVITY_LINES[1], all_passed(20)),
            ],
        );
        let mixed = fixture_response(
            "diag-fixture-mixed",
            vec![
                line_result(CONNECTIVITY_LINES[0], {
                    let mut layers = all_passed(10);
                    layers[3] = layer(
                        "api_key",
                        LayerStatus::Skipped,
                        None,
                        LayerReasonCode::SkippedNoSavedSession,
                    );
                    layers
                }),
                line_result(CONNECTIVITY_LINES[1], skipped_after_dns_failure()),
            ],
        );
        let failed = fixture_response(
            "diag-fixture-failed",
            vec![
                line_result(
                    CONNECTIVITY_LINES[0],
                    vec![
                        layer(
                            "dns",
                            LayerStatus::Passed,
                            Some(10),
                            LayerReasonCode::DnsResolved,
                        ),
                        layer(
                            "tcp",
                            LayerStatus::Failed,
                            Some(11),
                            LayerReasonCode::TcpConnectionFailed,
                        ),
                        layer(
                            "tls",
                            LayerStatus::Skipped,
                            None,
                            LayerReasonCode::SkippedUpstreamFailed,
                        ),
                        layer(
                            "api_key",
                            LayerStatus::Skipped,
                            None,
                            LayerReasonCode::SkippedUpstreamFailed,
                        ),
                    ],
                ),
                line_result(
                    CONNECTIVITY_LINES[1],
                    vec![
                        layer(
                            "dns",
                            LayerStatus::Failed,
                            Some(20),
                            LayerReasonCode::DnsLookupTimedOut,
                        ),
                        layer(
                            "tcp",
                            LayerStatus::Skipped,
                            None,
                            LayerReasonCode::SkippedUpstreamFailed,
                        ),
                        layer(
                            "tls",
                            LayerStatus::Skipped,
                            None,
                            LayerReasonCode::SkippedUpstreamFailed,
                        ),
                        layer(
                            "api_key",
                            LayerStatus::Skipped,
                            None,
                            LayerReasonCode::SkippedUpstreamFailed,
                        ),
                    ],
                ),
            ],
        );

        let actual = serde_json::json!({
            "reachable": reachable,
            "mixed": mixed,
            "failed": failed,
        });
        assert_eq!(actual, fixture);
    }

    #[test]
    fn diagnostics_contract_request_cannot_supply_network_targets_or_secrets() {
        let request: ConnectivityRequest =
            serde_json::from_str(r#"{"requestId":"diag-1"}"#).unwrap();
        assert_eq!(request.request_id, "diag-1");
        for field in ["host", "url", "port", "path", "headers", "apiKey", "proxy"] {
            let mut payload = serde_json::json!({"requestId": "diag-1"});
            payload[field] = serde_json::json!("not-accepted");
            assert!(serde_json::from_value::<ConnectivityRequest>(payload).is_err());
        }
    }
}
