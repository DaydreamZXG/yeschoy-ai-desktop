//! Shared bounds for AI requests accepted by the desktop's loopback gateways.
//!
//! Attachments are embedded in JSON and commonly grow after base64 encoding, so
//! every supported client must use the same request limit. Response and stream
//! limits remain owned by each protocol bridge and must not reuse this value.

use std::sync::{Arc, OnceLock};

use axum::{
    body::{to_bytes, Body},
    http::{header, HeaderMap},
};
use bytes::Bytes;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Matches the established main proxy limit while remaining explicitly bounded.
pub(crate) const MAX_AI_REQUEST_BODY_BYTES: usize = 200 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AiRequestAdmissionError {
    PayloadTooLarge,
    Busy,
}

#[derive(Debug)]
pub(crate) struct AiRequestPermit {
    _permit: OwnedSemaphorePermit,
}

struct AiRequestAdmission {
    slots: Arc<Semaphore>,
}

impl AiRequestAdmission {
    fn new() -> Self {
        // Production admits a single body at a time across every bridge, so
        // the 200 MiB per-request ceiling is also the process-wide ceiling.
        Self {
            slots: Arc::new(Semaphore::new(1)),
        }
    }

    #[cfg(test)]
    fn with_slots(slots: usize) -> Self {
        Self {
            slots: Arc::new(Semaphore::new(slots)),
        }
    }

    fn content_length(headers: &HeaderMap) -> Result<(), AiRequestAdmissionError> {
        if headers
            .get(header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|length| length > MAX_AI_REQUEST_BODY_BYTES as u64)
        {
            return Err(AiRequestAdmissionError::PayloadTooLarge);
        }
        Ok(())
    }

    async fn acquire(
        &self,
        headers: &HeaderMap,
    ) -> Result<AiRequestPermit, AiRequestAdmissionError> {
        Self::content_length(headers)?;
        self.slots
            .clone()
            .try_acquire_owned()
            .map(|permit| AiRequestPermit { _permit: permit })
            .map_err(|_| AiRequestAdmissionError::Busy)
    }

    #[cfg(test)]
    fn try_acquire_for_test(
        &self,
        headers: &HeaderMap,
    ) -> Result<AiRequestPermit, AiRequestAdmissionError> {
        Self::content_length(headers)?;
        self.slots
            .clone()
            .try_acquire_owned()
            .map(|permit| AiRequestPermit { _permit: permit })
            .map_err(|_| AiRequestAdmissionError::Busy)
    }
}

fn admission() -> &'static AiRequestAdmission {
    static ADMISSION: OnceLock<AiRequestAdmission> = OnceLock::new();
    #[cfg(not(test))]
    {
        ADMISSION.get_or_init(AiRequestAdmission::new)
    }
    #[cfg(test)]
    {
        ADMISSION.get_or_init(|| {
            // Rust's handler fixtures run hundreds of independent mock servers in
            // parallel inside one process. They receive parallel fixture
            // capacity here; the dedicated instance-level test below exercises
            // the exact production one-slot, fail-fast behavior.
            AiRequestAdmission::with_slots(1_024)
        })
    }
}

/// Call only after the request has passed the bridge's local authorization.
pub(crate) async fn admit_ai_request(
    headers: &HeaderMap,
) -> Result<AiRequestPermit, AiRequestAdmissionError> {
    admission().acquire(headers).await
}

pub(crate) async fn read_ai_request_body(body: Body) -> Result<Bytes, axum::Error> {
    read_body_with_limit(body, MAX_AI_REQUEST_BODY_BYTES).await
}

async fn read_body_with_limit(body: Body, limit: usize) -> Result<Bytes, axum::Error> {
    to_bytes(body, limit).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn accepts_attachment_requests_above_every_legacy_bridge_limit() {
        let body = vec![b'a'; 8 * 1024 * 1024 + 1];
        let bytes = read_ai_request_body(Body::from(body.clone()))
            .await
            .unwrap();
        assert_eq!(bytes.len(), body.len());
    }

    #[tokio::test]
    async fn remains_bounded_instead_of_reading_an_unlimited_body() {
        assert!(read_body_with_limit(Body::from(vec![0_u8; 17]), 16)
            .await
            .is_err());
    }

    #[test]
    fn rejects_declared_oversize_before_reading_and_competing_buffers_immediately() {
        let admission = AiRequestAdmission::new();
        let mut oversized = HeaderMap::new();
        oversized.insert(
            header::CONTENT_LENGTH,
            (MAX_AI_REQUEST_BODY_BYTES as u64 + 1)
                .to_string()
                .parse()
                .unwrap(),
        );
        assert_eq!(
            admission.try_acquire_for_test(&oversized).unwrap_err(),
            AiRequestAdmissionError::PayloadTooLarge
        );

        let first = admission.try_acquire_for_test(&HeaderMap::new()).unwrap();
        assert_eq!(
            admission
                .try_acquire_for_test(&HeaderMap::new())
                .unwrap_err(),
            AiRequestAdmissionError::Busy
        );
        drop(first);
        assert!(admission.try_acquire_for_test(&HeaderMap::new()).is_ok());
    }
}
