//! Request identity/shape tests moved with the request use case.
#![cfg(test)]

use super::*;
use serde_json::json;
#[test]
fn request_requires_one_of_six_exact_targets_and_selection_shape() {
    let valid = ToolActivationRequest {
        request_id: "activation-1".into(),
        line_id: "mainland_optimized".into(),
        tool_id: "pi".into(),
        model_id: "glm-5.3".into(),
        installation_id: "i0123456789abcdef".into(),
        billing_group: "default".into(),
        models: None,
        installation_job_id: None,
        restart_running_app: false,
        displace_claude_login: false,
        probe_codex_transport: false,
    };
    assert!(request_is_valid(&valid));
    let mut invalid = valid.clone();
    invalid.restart_running_app = true;
    assert!(!request_is_valid(&invalid));
    let mut invalid = valid;
    invalid.tool_id = "aider".into();
    assert!(!request_is_valid(&invalid));

    let comma = json!({"requestId":"fixture", "lineId":"mainland_optimized", "toolId":"pi", "modelId":"model,a", "billingGroup":"default", "installationId":"i0123456789abcdef"});
    let comma: ToolActivationRequest = serde_json::from_value(comma).unwrap();
    assert!(!request_is_valid(&comma));
}

#[test]
fn activation_accepts_station_connect_line_ids() {
    let mut valid = ToolActivationRequest {
        request_id: "activation-1".into(),
        line_id: "station_connect:https://example.com".into(),
        tool_id: "pi".into(),
        model_id: "glm-5.3".into(),
        installation_id: "i0123456789abcdef".into(),
        billing_group: "default".into(),
        models: None,
        installation_job_id: None,
        restart_running_app: false,
        displace_claude_login: false,
        probe_codex_transport: false,
    };
    assert!(request_is_valid(&valid));
    valid.line_id = "station_connect:http://example.com".into();
    assert!(!request_is_valid(&valid));
}

#[test]
fn codex_probe_is_opt_in_and_absent_payloads_stay_off() {
    let base = json!({"requestId":"fixture", "lineId":"mainland_optimized",
        "toolId":"codex_desktop", "modelId":"glm-5.3", "billingGroup":"default",
        "installationId":"i0123456789abcdef"});
    // The renderer omits the field unless the user turned the probe on; the
    // missing field must deserialize to "no probe", never to an implicit yes.
    let omitted: ToolActivationRequest = serde_json::from_value(base.clone()).unwrap();
    assert!(!omitted.probe_codex_transport);
    let mut opted_in = base;
    opted_in["probeCodexTransport"] = json!(true);
    let opted_in: ToolActivationRequest = serde_json::from_value(opted_in).unwrap();
    assert!(opted_in.probe_codex_transport);
    assert!(request_is_valid(&opted_in));
}
