import type { RequestObservation } from "./connections";
import { groupLabel } from "./BillingGroupPicker";
import { useConfigurationCopy } from "./copy";
import { useWorkbenchCopy } from "../workbench/copy";
import type { ActivationToolId } from "./activation";

export function RecentRequest({
  value,
  toolId,
  onRefresh,
  loading = false,
}: {
  value?: RequestObservation;
  toolId?: ActivationToolId;
  onRefresh?: () => void;
  loading?: boolean;
}) {
  const c = useConfigurationCopy();
  const w = useWorkbenchCopy();
  const outcomes: Record<RequestObservation["outcome"], string> = {
    ok: c.outcomeOk,
    timeout: c.outcomeTimeout,
    network_error: c.outcomeNetworkError,
    upstream_error: c.outcomeUpstreamError,
    invalid_response: c.outcomeInvalidResponse,
    stream_interrupted: c.outcomeStreamInterrupted,
    unknown_model: c.outcomeUnknownModel,
    payload_too_large: c.outcomePayloadTooLarge,
    local_busy: c.outcomeLocalBusy,
  };
  return (
    <section className="recent-request" aria-label={c.sectionLabel}>
      <header>
        <strong>{c.recentTitle}</strong>
        {onRefresh && (
          <button
            type="button"
            className="text-button"
            disabled={loading}
            onClick={onRefresh}
          >
            {c.refreshResult}
          </button>
        )}
      </header>
      {value ? (
        <>
          <p className="request-outcome" data-outcome={value.outcome}>
            {outcomes[value.outcome]}
            {value.httpStatus > 0 ? ` · HTTP ${value.httpStatus}` : ""}
          </p>
          <code>{value.modelId || c.noModelSpecified}</code>
          <small>
            {value.billingGroup
              ? `${groupLabel(value.billingGroup, c.defaultGroup)} · `
              : ""}
            {value.lineId === "global_accelerated"
              ? w.globalLine
              : w.mainlandLine}{" "}
            · {new Date(value.observedAtEpochMs).toLocaleTimeString()}
          </small>
          {value.outcome !== "ok" && (
            <p>
              {value.outcome === "payload_too_large"
                ? c.advicePayloadTooLarge
                : value.outcome === "local_busy"
                  ? c.adviceLocalBusy
                  : [401, 403].includes(value.httpStatus)
                    ? c.adviceUnauthorized
                    : value.httpStatus === 429
                      ? c.adviceRateLimited
                      : value.outcome === "unknown_model"
                        ? c.adviceUnknownModel
                        : c.adviceRetry}
            </p>
          )}
        </>
      ) : (
        <p>{c.emptyRequestState}</p>
      )}
      {toolId === "codex_desktop" && <p>{c.codexAccountNote}</p>}
      <small>{c.attributionNote}</small>
    </section>
  );
}
