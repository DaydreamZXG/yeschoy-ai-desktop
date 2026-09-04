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
  rootUrl: "https://yeschoy.com" | "https://yeschoy.pro";
  host: "yeschoy.com" | "yeschoy.pro";
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
    rootUrl: "https://yeschoy.pro",
    host: "yeschoy.pro",
    port: 443,
  },
] as const;

const REASON_BY_STATUS: Record<ConnectivityStatus, ConnectivityReasonCode> = {
  reachable: "tcp_443_reachable",
  dns_failed: "dns_resolution_failed",
  connect_failed: "tcp_connection_failed",
  timed_out: "connectivity_check_timed_out",
};

export function isCompleteConnectivityProjection(
  response: ConnectivityResponse,
  expectedRequestId: string,
): boolean {
  if (response.requestId !== expectedRequestId) return false;
  if (!Number.isInteger(response.startedAtEpochMs)) return false;
  if (!Number.isInteger(response.completedAtEpochMs)) return false;
  if (response.completedAtEpochMs < response.startedAtEpochMs) return false;
  if (response.lines.length !== CONNECTIVITY_LINES.length) return false;

  return CONNECTIVITY_LINES.every((expected) => {
    const candidates = response.lines.filter(
      (line) => line.lineId === expected.lineId,
    );
    if (candidates.length !== 1) return false;
    const line = candidates[0];
    return (
      line.displayName === expected.displayName &&
      line.rootUrl === expected.rootUrl &&
      line.host === expected.host &&
      line.port === expected.port &&
      Number.isInteger(line.latencyMs) &&
      line.latencyMs >= 0 &&
      line.latencyMs <= 60000 &&
      REASON_BY_STATUS[line.status] === line.reasonCode
    );
  });
}
