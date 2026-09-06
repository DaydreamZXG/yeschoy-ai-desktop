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

use crate::connectivity_core::{
    request_id_is_valid, ConnectivityLineResult, ConnectivityReasonCode, ConnectivityStatus,
    LineSpec, CONNECTIVITY_LINES,
};

const DNS_TIMEOUT: Duration = Duration::from_secs(3);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectivityRequest {
    request_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectivityResponse {
    request_id: String,
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

fn bounded_elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(60_000) as u64
}

fn result(
    line: LineSpec,
    status: ConnectivityStatus,
    reason_code: ConnectivityReasonCode,
    started: Instant,
) -> ConnectivityLineResult {
    ConnectivityLineResult {
        line_id: line.line_id,
        display_name: line.display_name,
        root_url: line.root_url,
        host: line.host,
        port: line.port,
        status,
        latency_ms: bounded_elapsed_ms(started),
        reason_code,
    }
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

async fn check_line(line: LineSpec) -> ConnectivityLineResult {
    let started = Instant::now();
    let addresses = match timeout(DNS_TIMEOUT, lookup_host((line.host, line.port))).await {
        Err(_) => {
            return result(
                line,
                ConnectivityStatus::TimedOut,
                ConnectivityReasonCode::ConnectivityCheckTimedOut,
                started,
            )
        }
        Ok(Err(_)) => {
            return result(
                line,
                ConnectivityStatus::DnsFailed,
                ConnectivityReasonCode::DnsResolutionFailed,
                started,
            )
        }
        Ok(Ok(addresses)) => addresses.collect::<Vec<_>>(),
    };

    if addresses.is_empty() {
        return result(
            line,
            ConnectivityStatus::DnsFailed,
            ConnectivityReasonCode::DnsResolutionFailed,
            started,
        );
    }

    match timeout(CONNECT_TIMEOUT, connect_any(&addresses)).await {
        Err(_) => result(
            line,
            ConnectivityStatus::TimedOut,
            ConnectivityReasonCode::ConnectivityCheckTimedOut,
            started,
        ),
        Ok(Err(_)) => result(
            line,
            ConnectivityStatus::ConnectFailed,
            ConnectivityReasonCode::TcpConnectionFailed,
            started,
        ),
        Ok(Ok(())) => result(
            line,
            ConnectivityStatus::Reachable,
            ConnectivityReasonCode::Tcp443Reachable,
            started,
        ),
    }
}

#[tauri::command]
pub async fn check_line_connectivity_read_only(
    request: ConnectivityRequest,
) -> Result<ConnectivityResponse, String> {
    if !request_id_is_valid(&request.request_id) {
        return Err("invalid_request_id".to_string());
    }

    let started_at_epoch_ms = unix_epoch_ms();
    let (mainland, global) = tokio::join!(
        check_line(CONNECTIVITY_LINES[0]),
        check_line(CONNECTIVITY_LINES[1])
    );

    Ok(ConnectivityResponse {
        request_id: request.request_id,
        started_at_epoch_ms,
        completed_at_epoch_ms: unix_epoch_ms(),
        lines: vec![mainland, global],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_response(
        request_id: &str,
        outcomes: [(ConnectivityStatus, ConnectivityReasonCode); 2],
    ) -> ConnectivityResponse {
        ConnectivityResponse {
            request_id: request_id.to_owned(),
            started_at_epoch_ms: 1_788_607_800_000,
            completed_at_epoch_ms: 1_788_607_800_030,
            lines: CONNECTIVITY_LINES
                .iter()
                .zip(outcomes)
                .enumerate()
                .map(
                    |(index, (line, (status, reason_code)))| ConnectivityLineResult {
                        line_id: line.line_id,
                        display_name: line.display_name,
                        root_url: line.root_url,
                        host: line.host,
                        port: line.port,
                        status,
                        latency_ms: (index as u64 + 1) * 10,
                        reason_code,
                    },
                )
                .collect(),
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
        let reachable = (
            ConnectivityStatus::Reachable,
            ConnectivityReasonCode::Tcp443Reachable,
        );
        let actual = serde_json::json!({
            "reachable": fixture_response("diag-fixture-reachable", [reachable, reachable]),
            "mixed": fixture_response("diag-fixture-mixed", [reachable, (
                ConnectivityStatus::DnsFailed,
                ConnectivityReasonCode::DnsResolutionFailed,
            )]),
            "failed": fixture_response("diag-fixture-failed", [(
                ConnectivityStatus::ConnectFailed,
                ConnectivityReasonCode::TcpConnectionFailed,
            ), (
                ConnectivityStatus::TimedOut,
                ConnectivityReasonCode::ConnectivityCheckTimedOut,
            )]),
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
