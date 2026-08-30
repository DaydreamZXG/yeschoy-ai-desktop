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
