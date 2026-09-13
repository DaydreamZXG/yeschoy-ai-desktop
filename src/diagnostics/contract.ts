export type ConnectivityLineId = "mainland_optimized" | "global_accelerated";

export type ConnectivityLayerId = "dns" | "tcp" | "tls" | "api_key";

export type LayerStatus = "passed" | "failed" | "skipped";

// Must match connectivity_core.rs LayerReasonCode (serde snake_case).
export type LayerReasonCode =
  | "dns_resolved"
  | "dns_resolution_failed"
  | "dns_lookup_timed_out"
  | "tcp443_reachable"
  | "tcp_connection_failed"
  | "tcp_connect_timed_out"
  | "tls_handshake_verified"
  | "tls_certificate_invalid"
  | "tls_handshake_failed"
  | "tls_handshake_timed_out"
  | "session_token_valid"
  | "session_token_rejected"
  | "api_probe_error"
  | "skipped_upstream_failed"
  | "skipped_no_saved_session"
  | "session_probe_unavailable";

export interface ConnectivityLayerResult {
  layer: ConnectivityLayerId;
  status: LayerStatus;
  latencyMs?: number;
  reasonCode: LayerReasonCode;
}

export interface ConnectivityLineResult {
  lineId: ConnectivityLineId;
  displayName: "大陆优化" | "全球加速";
  rootUrl: "https://yeschoy.com" | "https://api.yeschoy.com";
  host: "yeschoy.com" | "api.yeschoy.com";
  port: 443;
  layers: ConnectivityLayerResult[];
}

export interface ConnectivityResponse {
  requestId: string;
  schemaVersion: 2;
  startedAtEpochMs: number;
  completedAtEpochMs: number;
  lines: ConnectivityLineResult[];
}

export const CONNECTIVITY_LINES = [
  {
    lineId: "mainland_optimized",
    displayName: "大陆优化",
    rootUrl: "https://yeschoy.com",
    host: "yeschoy.com",
    port: 443,
  },
  {
    lineId: "global_accelerated",
    displayName: "全球加速",
    rootUrl: "https://api.yeschoy.com",
    host: "api.yeschoy.com",
    port: 443,
  },
] as const;

// Mirrors connectivity_core.rs CONNECTIVITY_LAYER_IDS (order = execution order).
export const CONNECTIVITY_LAYER_IDS = [
  "dns",
  "tcp",
  "tls",
  "api_key",
] as const satisfies readonly ConnectivityLayerId[];

type ReasonConstraint = { status: LayerStatus; layer: ConnectivityLayerId | "any_non_dns" };

const REASON_CONSTRAINTS: Record<LayerReasonCode, ReasonConstraint> = {
  dns_resolved: { status: "passed", layer: "dns" },
  dns_resolution_failed: { status: "failed", layer: "dns" },
  dns_lookup_timed_out: { status: "failed", layer: "dns" },
  tcp443_reachable: { status: "passed", layer: "tcp" },
  tcp_connection_failed: { status: "failed", layer: "tcp" },
  tcp_connect_timed_out: { status: "failed", layer: "tcp" },
  tls_handshake_verified: { status: "passed", layer: "tls" },
  tls_certificate_invalid: { status: "failed", layer: "tls" },
  tls_handshake_failed: { status: "failed", layer: "tls" },
  tls_handshake_timed_out: { status: "failed", layer: "tls" },
  session_token_valid: { status: "passed", layer: "api_key" },
  session_token_rejected: { status: "failed", layer: "api_key" },
  api_probe_error: { status: "failed", layer: "api_key" },
  skipped_upstream_failed: { status: "skipped", layer: "any_non_dns" },
  skipped_no_saved_session: { status: "skipped", layer: "api_key" },
  session_probe_unavailable: { status: "skipped", layer: "api_key" },
};

function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function timestamp(value: unknown): value is number {
  return (
    Number.isSafeInteger(value) &&
    Number(value) >= 0 &&
    Number(value) <= 8_640_000_000_000_000
  );
}

function latency(value: unknown): value is number {
  return (
    Number.isSafeInteger(value) && Number(value) >= 0 && Number(value) <= 60_000
  );
}

function layerMatches(value: Record<string, unknown>): boolean {
  const keys = Object.keys(value);
  const hasRequired =
    keys.includes("layer") &&
    keys.includes("status") &&
    keys.includes("reasonCode");
  if (!hasRequired || keys.some((key) => !["layer", "status", "latencyMs", "reasonCode"].includes(key)))
    return false;
  if (typeof value.layer !== "string" || !(CONNECTIVITY_LAYER_IDS as readonly string[]).includes(value.layer))
    return false;
  if (typeof value.reasonCode !== "string" || !(value.reasonCode in REASON_CONSTRAINTS))
    return false;
  const constraint = REASON_CONSTRAINTS[value.reasonCode as LayerReasonCode];
  if (value.status !== constraint.status) return false;
  if (
    constraint.layer !== "any_non_dns" &&
    value.layer !== constraint.layer
  )
    return false;
  if (constraint.layer === "any_non_dns" && value.layer === "dns") return false;
  return (
    !("latencyMs" in value) ||
    (latency(value.latencyMs) &&
      (value.status === "skipped" ? value.latencyMs === undefined : true))
  );
}

function lineMatches(
  value: Record<string, unknown>,
  expected: (typeof CONNECTIVITY_LINES)[number],
): boolean {
  if (
    Object.keys(value).length !== 6 ||
    value.lineId !== expected.lineId ||
    value.displayName !== expected.displayName ||
    value.rootUrl !== expected.rootUrl ||
    value.host !== expected.host ||
    value.port !== expected.port ||
    !Array.isArray(value.layers) ||
    value.layers.length !== CONNECTIVITY_LAYER_IDS.length
  )
    return false;
  return value.layers.every((layer, index) => {
    if (!record(layer)) return false;
    if (layer.layer !== CONNECTIVITY_LAYER_IDS[index]) return false;
    return layerMatches(layer);
  });
}

export interface DecodedConnectivityProjection {
  response: ConnectivityResponse;
  complete: boolean;
}

// A malformed native reply is not a network outcome. Only independently
// validated line evidence crosses this boundary; missing/ambiguous lines stay unknown.
export function decodeConnectivityProjection(
  value: unknown,
  expectedRequestId: string,
): DecodedConnectivityProjection | null {
  if (
    !/^[A-Za-z0-9_-]{1,64}$/.test(expectedRequestId) ||
    !record(value) ||
    Object.keys(value).length !== 5 ||
    !("requestId" in value) ||
    !("schemaVersion" in value) ||
    !("startedAtEpochMs" in value) ||
    !("completedAtEpochMs" in value) ||
    !("lines" in value) ||
    value.requestId !== expectedRequestId ||
    value.schemaVersion !== 2 ||
    !timestamp(value.startedAtEpochMs) ||
    !timestamp(value.completedAtEpochMs) ||
    value.completedAtEpochMs < value.startedAtEpochMs ||
    !Array.isArray(value.lines)
  )
    return null;

  const lines: ConnectivityLineResult[] = [];
  for (const expected of CONNECTIVITY_LINES) {
    const candidates = value.lines.filter(
      (line): line is Record<string, unknown> =>
        record(line) && line.lineId === expected.lineId,
    );
    if (candidates.length !== 1 || !lineMatches(candidates[0], expected))
      continue;
    lines.push(candidates[0] as unknown as ConnectivityLineResult);
  }
  return {
    response: {
      requestId: expectedRequestId,
      schemaVersion: 2,
      startedAtEpochMs: value.startedAtEpochMs,
      completedAtEpochMs: value.completedAtEpochMs,
      lines,
    },
    complete:
      value.lines.length === CONNECTIVITY_LINES.length &&
      lines.length === CONNECTIVITY_LINES.length,
  };
}

export function isCompleteConnectivityProjection(
  response: unknown,
  expectedRequestId: string,
): response is ConnectivityResponse {
  return (
    decodeConnectivityProjection(response, expectedRequestId)?.complete === true
  );
}

// Derives the headline outcome of one line from its layer evidence.
export type LineOutcome =
  | "reachable"
  | "dns_failed"
  | "connect_failed"
  | "tls_failed"
  | "api_key_failed"
  | "timed_out";

export function lineOutcome(line: ConnectivityLineResult): LineOutcome {
  const firstFailed = line.layers.find((layer) => layer.status === "failed");
  if (!firstFailed) return "reachable";
  if (firstFailed.reasonCode.endsWith("_timed_out")) return "timed_out";
  switch (firstFailed.layer) {
    case "dns":
      return "dns_failed";
    case "tcp":
      return "connect_failed";
    case "tls":
      return "tls_failed";
    default:
      return "api_key_failed";
  }
}

// Network layers only (dns/tcp/tls); the api_key layer does not affect reachability.
export function lineNetworkHealthy(line: ConnectivityLineResult): boolean {
  return line.layers
    .filter((layer) => layer.layer !== "api_key")
    .every((layer) => layer.status === "passed");
}
