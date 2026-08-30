export const ACCOUNT_READINESS_STATUS = "backend_upgrade_required" as const;

export const REMOTE_CAPABILITY_STATUS = "unavailable" as const;

export const COMPARISON_FX = Object.freeze({
  usdToCny: "6.75" as const,
  purpose: "display_only" as const,
  liveRate: false as const,
});

export interface AccountReadinessProjection {
  requestId: string;
  schemaVersion: 1;
  status: typeof ACCOUNT_READINESS_STATUS;
  networkAttempted: false;
  serverNamespace: "/api/desktop/v1";
  authorization: typeof REMOTE_CAPABILITY_STATUS;
  accountSummary: typeof REMOTE_CAPABILITY_STATUS;
  usage: typeof REMOTE_CAPABILITY_STATUS;
  pricing: typeof REMOTE_CAPABILITY_STATUS;
  recharge: typeof REMOTE_CAPABILITY_STATUS;
  comparisonFx: typeof COMPARISON_FX;
}

const REQUEST_ID_PATTERN = /^[A-Za-z0-9_-]{1,64}$/;

export function createAccountReadiness(
  requestId: string,
): AccountReadinessProjection {
  if (!REQUEST_ID_PATTERN.test(requestId)) {
    throw new Error("invalid_request_id");
  }

  return Object.freeze({
    requestId,
    schemaVersion: 1,
    status: ACCOUNT_READINESS_STATUS,
    networkAttempted: false,
    serverNamespace: "/api/desktop/v1",
    authorization: REMOTE_CAPABILITY_STATUS,
    accountSummary: REMOTE_CAPABILITY_STATUS,
    usage: REMOTE_CAPABILITY_STATUS,
    pricing: REMOTE_CAPABILITY_STATUS,
    recharge: REMOTE_CAPABILITY_STATUS,
    comparisonFx: COMPARISON_FX,
  });
}
