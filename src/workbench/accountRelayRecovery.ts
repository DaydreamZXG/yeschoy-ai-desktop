import type { AccountStatus } from "../account/session";
import type { AccountLineId, ConfigurationLineId } from "../configuration/preview";

const RELAY_LINE_RETRY_STATUSES = new Set<AccountStatus>([
  "backend_unavailable",
  "network_error",
  "incompatible_server",
]);

export function accountStatusSuggestsRelayLineRetry(
  status?: AccountStatus | null,
): boolean {
  return !!status && RELAY_LINE_RETRY_STATUSES.has(status);
}

export function isRelayCatalogLine(
  lineId: AccountLineId,
): lineId is ConfigurationLineId {
  return lineId === "mainland_optimized" || lineId === "global_accelerated";
}

export function alternateRelayLine(
  lineId: ConfigurationLineId,
): ConfigurationLineId {
  return lineId === "mainland_optimized"
    ? "global_accelerated"
    : "mainland_optimized";
}
