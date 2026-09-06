export type ConnectivityLineId = "mainland_optimized" | "global_accelerated";

export type ConnectivityStatus =
  | "reachable"
  | "dns_failed"
  | "connect_failed"
  | "timed_out";

export type ConnectivityReasonCode =
  | "tcp_443_reachable"
  | "dns_resolution_failed"
  | "tcp_connection_failed"
  | "connectivity_check_timed_out";

export interface ConnectivityLineResult {
  lineId: ConnectivityLineId;
  displayName: "大陆优化" | "全球加速";
  rootUrl: "https://yeschoy.com" | "https://api.yeschoy.com";
  host: "yeschoy.com" | "api.yeschoy.com";
  port: 443;
  status: ConnectivityStatus;
  latencyMs: number;
  reasonCode: ConnectivityReasonCode;
}

export interface ConnectivityResponse {
  requestId: string;
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

const REASON_BY_STATUS: Record<ConnectivityStatus, ConnectivityReasonCode> = {
  reachable: "tcp_443_reachable",
  dns_failed: "dns_resolution_failed",
  connect_failed: "tcp_connection_failed",
  timed_out: "connectivity_check_timed_out",
};

function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function exactKeys(value: Record<string, unknown>, keys: string[]): boolean {
  return (
    Object.keys(value).length === keys.length &&
    keys.every((key) => Object.prototype.hasOwnProperty.call(value, key))
  );
}

function timestamp(value: unknown): value is number {
  return (
    Number.isSafeInteger(value) &&
    Number(value) >= 0 &&
    Number(value) <= 8_640_000_000_000_000
  );
}

function lineMatches(
  value: Record<string, unknown>,
  expected: (typeof CONNECTIVITY_LINES)[number],
): boolean {
  return (
    exactKeys(value, [
      "lineId",
      "displayName",
      "rootUrl",
      "host",
      "port",
      "status",
      "latencyMs",
      "reasonCode",
    ]) &&
    value.displayName === expected.displayName &&
    value.rootUrl === expected.rootUrl &&
    value.host === expected.host &&
    value.port === expected.port &&
    Number.isSafeInteger(value.latencyMs) &&
    Number(value.latencyMs) >= 0 &&
    Number(value.latencyMs) <= 60_000 &&
    typeof value.status === "string" &&
    Object.prototype.hasOwnProperty.call(REASON_BY_STATUS, value.status) &&
    REASON_BY_STATUS[value.status as ConnectivityStatus] === value.reasonCode
  );
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
    !exactKeys(value, [
      "requestId",
      "startedAtEpochMs",
      "completedAtEpochMs",
      "lines",
    ]) ||
    value.requestId !== expectedRequestId ||
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
    const line = candidates[0];
    lines.push({
      ...expected,
      status: line.status as ConnectivityStatus,
      latencyMs: line.latencyMs as number,
      reasonCode: line.reasonCode as ConnectivityReasonCode,
    });
  }
  return {
    response: {
      requestId: expectedRequestId,
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
