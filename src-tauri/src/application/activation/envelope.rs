//! One owner for the relay's account-envelope rules, used by model capability and
//! token enrollment. Free-form `message` text is never logged or exposed.
use serde_json::Value;

// The relay answers every account endpoint with this same envelope.
pub(crate) fn data(value: &Value) -> Option<&Value> {
    let object = value.as_object()?;
    object
        .get("success")?
        .as_bool()?
        .then(|| object.get("data"))
        .flatten()
}

pub(crate) fn server_success(status: u16, value: &Value) -> bool {
    (200..300).contains(&status)
        && value
            .as_object()
            .and_then(|object| object.get("success"))
            .and_then(Value::as_bool)
            == Some(true)
}

/// A body that explicitly says `success: false` is a rejection even on HTTP 200.
/// Kept for adapters that must distinguish a business refusal from a transport
/// failure; it is exercised by the tests below and by token tests.
#[allow(dead_code)]
pub(crate) fn body_reports_failure(value: &Value) -> bool {
    value
        .as_object()
        .and_then(|object| object.get("success"))
        .and_then(Value::as_bool)
        == Some(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn envelope_accepts_only_explicit_success_and_never_needs_server_text() {
        assert!(server_success(200, &json!({"success": true, "data": []})));
        assert!(!server_success(
            200,
            &json!({"success": false, "message": "sk-leaked-key"})
        ));
        assert!(!server_success(401, &json!({"success": true})));
        assert_eq!(
            data(&json!({"success": true, "data": {"items": []}})),
            Some(&json!({"items": []}))
        );
        assert!(body_reports_failure(
            &json!({"success": false, "message": "synthetic"})
        ));
        assert!(!body_reports_failure(
            &json!({"message": "no success flag"})
        ));
    }
}
