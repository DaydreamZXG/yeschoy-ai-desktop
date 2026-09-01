import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { COMPARISON_FX } from "../account/readiness";
import type {
  ConfigurationLineId,
  ConfigurationToolId,
} from "../configuration/preview";
import { isServiceCatalog } from "./contract";
import type { ServiceCatalog } from "./contract";
import { createToolAccessPlan } from "./access-plan";
import type { ToolAccessPlan } from "./access-plan";

interface Props {
  toolId: ConfigurationToolId;
  lineId: ConfigurationLineId;
  onPlanChange: (plan: ToolAccessPlan | null) => void;
  onReadAttempt: () => void;
}

export function ServiceCatalogPanel({
  toolId,
  lineId,
  onPlanChange,
  onReadAttempt,
}: Props) {
  const { t, i18n } = useTranslation();
  const [catalog, setCatalog] = useState<ServiceCatalog | null>(null);
  const [phase, setPhase] = useState<"idle" | "loading" | "loaded" | "error">(
    "idle",
  );
  const [failure, setFailure] = useState("invalid_response");
  const [groupId, setGroupId] = useState("");
  const [modelId, setModelId] = useState("");
  const [search, setSearch] = useState("");
  const latest = useRef("");
  const sequence = useRef(0);
  const currentLine = useRef(lineId);
  currentLine.current = lineId;

  useEffect(() => {
    latest.current = "";
    setCatalog(null);
    setGroupId("");
    setModelId("");
    setSearch("");
    setPhase("idle");
    onPlanChange(null);
    return () => {
      latest.current = "";
    };
  }, [lineId, onPlanChange]);

  const currentCatalog = catalog?.lineId === lineId ? catalog : null;
  const models =
    currentCatalog?.models.filter((model) => model.groups.includes(groupId)) ??
    [];
  const filtered = models.filter((model) =>
    model.id.toLocaleLowerCase().includes(search.toLocaleLowerCase()),
  );
  const plan = useMemo(
    () =>
      currentCatalog?.catalogStatus === "available" && groupId && modelId
        ? createToolAccessPlan(currentCatalog, toolId, lineId, groupId, modelId)
        : null,
    [currentCatalog, toolId, lineId, groupId, modelId],
  );
  useEffect(() => {
    onPlanChange(plan);
  }, [onPlanChange, plan]);

  async function readCatalog() {
    const requestId = `catalog-${Date.now().toString(36)}-${++sequence.current}`;
    const requestedLine = lineId;
    latest.current = requestId;
    setCatalog(null);
    setGroupId("");
    setModelId("");
    setSearch("");
    setPhase("loading");
    onPlanChange(null);
    onReadAttempt();
    try {
      const result: unknown = await invoke("read_public_service_catalog", {
        request: { requestId, lineId: requestedLine },
      });
      if (latest.current !== requestId || currentLine.current !== requestedLine)
        return;
      if (!isServiceCatalog(result, requestId, requestedLine))
        throw new Error("invalid_response");
      setCatalog(result);
      setPhase("loaded");
    } catch (cause) {
      if (latest.current !== requestId || currentLine.current !== requestedLine)
        return;
      const message = cause instanceof Error ? cause.message : cause;
      setFailure(
        message === "catalog_busy"
          ? "catalog_busy"
          : message === "invalid_response"
            ? "invalid_response"
            : "client_unavailable",
      );
      setCatalog(null);
      setPhase("error");
    }
  }

  const catalogError =
    currentCatalog?.catalogStatus === "unavailable"
      ? currentCatalog.catalogError
      : null;
  const group = currentCatalog?.groups.find((group) => group.id === groupId);
  return (
    <section
      className="service-catalog-panel"
      aria-labelledby="service-catalog-title"
      aria-busy={phase === "loading"}
      data-phase={phase}
    >
      <div className="catalog-heading">
        <div>
          <p className="eyebrow">{t("yeschoyCatalog.eyebrow")}</p>
          <h2 id="service-catalog-title">{t("yeschoyCatalog.title")}</h2>
        </div>
        <button
          type="button"
          className="catalog-read-button"
          onClick={readCatalog}
          disabled={phase === "loading"}
        >
          {t(
            phase === "loading"
              ? "yeschoyCatalog.loading"
              : phase === "idle"
                ? "yeschoyCatalog.read"
                : "yeschoyCatalog.refresh",
          )}
        </button>
      </div>
      <p className="catalog-boundary">{t("yeschoyCatalog.publicNotice")}</p>
      {phase === "loading" && (
        <p role="status">{t("yeschoyCatalog.loadingNote")}</p>
      )}
      {(phase === "error" || catalogError) && (
        <p className="catalog-error" role="alert">
          {t(`yeschoyCatalog.errors.${catalogError ?? failure}`)}
        </p>
      )}
      {currentCatalog && (
        <div className="catalog-source" role="status">
          <span>
            {t("yeschoyCatalog.observed", {
              time: new Intl.DateTimeFormat(i18n.resolvedLanguage, {
                hour: "2-digit",
                minute: "2-digit",
                second: "2-digit",
              }).format(currentCatalog.observedAtEpochMs),
            })}
          </span>
          <span>
            {t(
              `yeschoyCatalog.backend.${currentCatalog.desktopBackend.status}`,
            )}
          </span>
        </div>
      )}
      {currentCatalog?.catalogStatus === "available" && (
        <>
          {currentCatalog.groups.length === 0 ||
          currentCatalog.models.length === 0 ? (
            <p className="catalog-empty" role="status">
              {t("yeschoyCatalog.empty")}
            </p>
          ) : (
            <div className="catalog-fields">
              <label htmlFor="catalog-group">
                {t("yeschoyCatalog.groupLabel")}
                <select
                  id="catalog-group"
                  value={groupId}
                  onChange={(event) => {
                    setGroupId(event.target.value);
                    setModelId("");
                    setSearch("");
                    onPlanChange(null);
                  }}
                >
                  <option value="">{t("yeschoyCatalog.chooseGroup")}</option>
                  {currentCatalog.groups.map((group) => (
                    <option key={group.id} value={group.id}>
                      {group.id}
                    </option>
                  ))}
                </select>
                <small>{t("yeschoyCatalog.groupHelp")}</small>
              </label>
              {group && (
                <p className="catalog-group-note">
                  {group.description || t("yeschoyCatalog.noGroupDescription")}
                  <small>
                    {t("yeschoyCatalog.multiplier", {
                      value: group.multiplier,
                    })}
                  </small>
                </p>
              )}
              <label htmlFor="catalog-search">
                {t("yeschoyCatalog.searchLabel")}
                <input
                  id="catalog-search"
                  type="search"
                  value={search}
                  disabled={!groupId}
                  onChange={(event) => setSearch(event.target.value)}
                  placeholder={t("yeschoyCatalog.searchPlaceholder")}
                />
              </label>
              <label htmlFor="catalog-model">
                {t("yeschoyCatalog.modelLabel")}
                <select
                  id="catalog-model"
                  value={modelId}
                  disabled={!groupId}
                  onChange={(event) => setModelId(event.target.value)}
                >
                  <option value="">{t("yeschoyCatalog.chooseModel")}</option>
                  {modelId &&
                    !filtered.some((model) => model.id === modelId) && (
                      <option value={modelId}>{modelId}</option>
                    )}
                  {filtered.map((model) => (
                    <option key={model.id} value={model.id}>
                      {model.id}
                    </option>
                  ))}
                </select>
                <small>
                  {t("yeschoyCatalog.modelCount", { count: filtered.length })}
                </small>
              </label>
            </div>
          )}
          {plan && (
            <div
              className="catalog-access-plan"
              data-status={plan.status}
              role="status"
            >
              <strong>{t(`yeschoyCatalog.protocol.${plan.status}`)}</strong>
              <dl>
                <div>
                  <dt>{t("yeschoyCatalog.modelLabel")}</dt>
                  <dd>
                    <code>{plan.modelId}</code>
                  </dd>
                </div>
                <div>
                  <dt>{t("yeschoyCatalog.billingLabel")}</dt>
                  <dd>{t(`yeschoyCatalog.billing.${plan.billingMode}`)}</dd>
                </div>
              </dl>
              <p>{t("yeschoyCatalog.compatibilityNotice")}</p>
              {plan.requiredProtocol && (
                <details>
                  <summary>{t("yeschoyCatalog.requiredProtocol")}</summary>
                  <code>{plan.requiredProtocol}</code>
                </details>
              )}
            </div>
          )}
          <details className="catalog-price-boundary">
            <summary>{t("yeschoyCatalog.priceTitle")}</summary>
            <p>
              {t("yeschoyCatalog.priceNotice", {
                comparisonFx: COMPARISON_FX.usdToCny,
                backendFx:
                  currentCatalog.backendDisplayExchangeRate ||
                  t("yeschoyCatalog.unknown"),
              })}
            </p>
          </details>
        </>
      )}
    </section>
  );
}
