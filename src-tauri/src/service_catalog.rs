use std::{
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::{
    header::{ACCEPT, CONTENT_TYPE},
    redirect::Policy,
    Client, StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Semaphore;

use crate::{
    connectivity_core::{request_id_is_valid, CONNECTIVITY_LINES},
    service_catalog_core::{
        parse_desktop_bootstrap, parse_public_catalog, unavailable_backend, CatalogError,
        DesktopBackend, PublicGroup, PublicModel,
    },
};

// Defensive local limits, not upstream capacity or latency guarantees.
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
static ACTIVE_READ: Semaphore = Semaphore::const_new(1);
static HTTP_CLIENT: OnceLock<Result<Client, CatalogError>> = OnceLock::new();

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServiceCatalogRequest {
    request_id: String,
    line_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceCatalogResponse {
    request_id: String,
    line_id: String,
    observed_at_epoch_ms: u64,
    catalog_status: &'static str,
    catalog_error: CatalogError,
    service_version: String,
    backend_display_exchange_rate: String,
    groups: Vec<PublicGroup>,
    models: Vec<PublicModel>,
    desktop_backend: DesktopBackend,
    user_specific: bool,
    secrets_accessed: bool,
}

fn build_client() -> Result<Client, CatalogError> {
    Client::builder()
        .https_only(true)
        .redirect(Policy::none())
        .referer(false)
        .no_proxy()
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(Duration::from_secs(5))
        .user_agent(concat!("YesChoyDesktop/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| CatalogError::NetworkError)
}

fn network_error(error: reqwest::Error) -> CatalogError {
    if error.is_timeout() {
        CatalogError::TimedOut
    } else {
        CatalogError::NetworkError
    }
}

async fn read_json(client: &Client, url: &str) -> Result<Option<Value>, CatalogError> {
    let mut response = client
        .get(url)
        .header(ACCEPT, "application/json")
        .send()
        .await
        .map_err(network_error)?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(CatalogError::HttpError);
    }
    let json_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/json")
        });
    if !json_type {
        return Err(CatalogError::InvalidResponse);
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(CatalogError::ResponseTooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(network_error)? {
        if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(body.len()) {
            return Err(CatalogError::ResponseTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|_| CatalogError::InvalidResponse)
}

#[tauri::command]
pub async fn read_public_service_catalog(
    request: ServiceCatalogRequest,
) -> Result<ServiceCatalogResponse, String> {
    if !request_id_is_valid(&request.request_id) {
        return Err("invalid_request_id".into());
    }
    let line = CONNECTIVITY_LINES
        .iter()
        .find(|line| line.line_id == request.line_id)
        .ok_or_else(|| "invalid_line_id".to_owned())?;
    let _permit = ACTIVE_READ
        .try_acquire()
        .map_err(|_| "catalog_busy".to_owned())?;
    let client = HTTP_CLIENT
        .get_or_init(build_client)
        .as_ref()
        .map_err(|_| "catalog_unavailable".to_owned())?;
    let status_url = format!("{}/api/status", line.root_url);
    let pricing_url = format!("{}/api/pricing", line.root_url);
    let bootstrap_url = format!("{}/api/desktop/v1/bootstrap", line.root_url);
    let (status, pricing, bootstrap) = tokio::join!(
        read_json(client, &status_url),
        read_json(client, &pricing_url),
        read_json(client, &bootstrap_url),
    );
    let desktop_backend = match bootstrap {
        Ok(None) => unavailable_backend("not_deployed", CatalogError::None),
        Ok(Some(value)) => parse_desktop_bootstrap(&value),
        Err(error) => unavailable_backend("unavailable", error),
    };
    let catalog = match (status, pricing) {
        (Ok(Some(status)), Ok(Some(pricing))) => parse_public_catalog(&status, &pricing),
        (Err(error), _) | (_, Err(error)) => Err(error),
        _ => Err(CatalogError::HttpError),
    };
    let mut response = ServiceCatalogResponse {
        request_id: request.request_id,
        line_id: request.line_id,
        observed_at_epoch_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u64::MAX as u128) as u64,
        catalog_status: "unavailable",
        catalog_error: CatalogError::None,
        service_version: String::new(),
        backend_display_exchange_rate: String::new(),
        groups: Vec::new(),
        models: Vec::new(),
        desktop_backend,
        user_specific: false,
        secrets_accessed: false,
    };
    match catalog {
        Ok(catalog) => {
            response.catalog_status = "available";
            response.service_version = catalog.service_version;
            response.backend_display_exchange_rate = catalog.backend_display_exchange_rate;
            response.groups = catalog.groups;
            response.models = catalog.models;
        }
        Err(error) => response.catalog_error = error,
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    async fn server(reply: String) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/api/pricing", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move {
            let (mut connection, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 4096];
            let length = connection.read(&mut request).await.unwrap();
            connection.write_all(reply.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&request[..length]).into_owned()
        });
        (url, handle)
    }

    #[tokio::test]
    async fn http_reader_checks_types_status_and_never_sends_credentials() {
        let client = Client::builder()
            .redirect(Policy::none())
            .no_proxy()
            .build()
            .unwrap();
        let (url, request) = server("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".into()).await;
        assert_eq!(
            read_json(&client, &url).await.unwrap(),
            Some(serde_json::json!({}))
        );
        let request = request.await.unwrap().to_ascii_lowercase();
        assert!(request.starts_with("get /api/pricing "));
        assert!(!request.contains("authorization:"));
        assert!(!request.contains("cookie:"));
        for (reply, expected) in [
            ("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 2\r\n\r\n{}".to_owned(), CatalogError::InvalidResponse),
            ("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/private\r\nContent-Length: 0\r\n\r\n".to_owned(), CatalogError::HttpError),
            (format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", MAX_RESPONSE_BYTES + 1), CatalogError::ResponseTooLarge),
        ] {
            let (url, server) = server(reply).await;
            assert_eq!(read_json(&client, &url).await.unwrap_err(), expected);
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn production_client_rejects_http_and_invalid_caller_inputs() {
        assert!(build_client()
            .unwrap()
            .get("http://127.0.0.1:9")
            .send()
            .await
            .is_err());
        for request in [
            ServiceCatalogRequest {
                request_id: "../secret".into(),
                line_id: "mainland_optimized".into(),
            },
            ServiceCatalogRequest {
                request_id: "safe".into(),
                line_id: "https://other.example".into(),
            },
        ] {
            assert!(read_public_service_catalog(request).await.is_err());
        }
        assert!(serde_json::from_str::<ServiceCatalogRequest>(
            r#"{"requestId":"a","lineId":"mainland_optimized","url":"https://other.example"}"#
        )
        .is_err());
    }
}
