import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDownUp, BadgePercent, Layers3 } from "lucide-react";
import { useAccountSession } from "../account/useAccountSession";
import { ServiceCatalogPanel } from "../service-catalog/ServiceCatalogPanel";
import {
  CONFIGURATION_LINES,
  type ConfigurationLineId,
  type ConfigurationToolId,
} from "../configuration/preview";
import type { ToolAccessPlan } from "../service-catalog/access-plan";
import { useWorkbenchCopy } from "./copy";
import { WorkbenchFooter } from "./WorkbenchChrome";

const noReadSideEffect = () => {};
const EMPTY_MODELS: never[] = [];

function price(value: string): string {
  const number = Number(value);
  return Number.isFinite(number) ? `¥${number.toFixed(2)}` : "—";
}

export function ModelsView() {
  const c = useWorkbenchCopy();
  const { t } = useTranslation();
  const [tool, setTool] = useState<ConfigurationToolId>("claude");
  const [line, setLine] =
    useState<ConfigurationLineId>("mainland_optimized");
  const [plan, setPlan] = useState<ToolAccessPlan | null>(null);
  const [selectedModelId, setSelectedModelId] = useState("");
  const { projection, loading, refresh } = useAccountSession(line);
  const accountModels =
    projection?.status === "signed_in" ? projection.models : EMPTY_MODELS;

  useEffect(() => {
    if (plan && accountModels.some((model) => model.id === plan.modelId)) {
      setSelectedModelId(plan.modelId);
      return;
    }
    if (!accountModels.some((model) => model.id === selectedModelId))
      setSelectedModelId(accountModels[0]?.id ?? "");
  }, [accountModels, plan, selectedModelId]);

  const selected = accountModels.find((model) => model.id === selectedModelId);
  const savings = useMemo(() => {
    if (!selected?.pricingAvailable) return null;
    const official = Number(selected.officialInputCnyPerMillion);
    const actual = Number(selected.actualInputCnyPerMillion);
    if (!Number.isFinite(official) || !Number.isFinite(actual) || official <= 0)
      return null;
    return Math.max(0, ((official - actual) / official) * 100);
  }, [selected]);

  return (
    <div className="workbench-page models-workspace">
      <header className="workbench-page-heading">
        <div>
          <h1>{c.models}</h1>
          <p>{c.priceBody}</p>
        </div>
        <span className="heading-symbol">
          <Layers3 aria-hidden="true" />
        </span>
      </header>

      <section className="model-browser">
        <div className="model-filters">
          <label>
            {c.selectApp}
            <select
              value={tool}
              onChange={(event) =>
                setTool(event.target.value as ConfigurationToolId)
              }
            >
              <option value="claude">Claude Desktop</option>
              <option value="codex">Codex</option>
            </select>
          </label>
          <label>
            {c.selectLine}
            <select
              value={line}
              onChange={(event) =>
                setLine(event.target.value as ConfigurationLineId)
              }
            >
              {CONFIGURATION_LINES.map((item) => (
                <option key={item.id} value={item.id}>
                  {t(`yeschoyConfiguration.lines.${item.id}.name`)}
                </option>
              ))}
            </select>
          </label>
        </div>
        <ServiceCatalogPanel
          toolId={tool}
          lineId={line}
          onPlanChange={setPlan}
          onReadAttempt={noReadSideEffect}
        />
      </section>

      <section className="price-comparison-panel account-price-panel">
        <div className="workbench-section-heading">
          <h2>
            <ArrowDownUp aria-hidden="true" />
            {c.officialPrice} / {c.actualPrice}
          </h2>
          {savings !== null && savings > 0 ? (
            <span className="savings-badge">
              <BadgePercent aria-hidden="true" />
              {c.saveCompared} {savings.toFixed(0)}%
            </span>
          ) : (
            <span className="status-badge">
              {loading ? c.checking : c.priceUnavailable}
            </span>
          )}
        </div>

        {projection?.status !== "signed_in" ? (
          <div className="price-account-empty">
            <p>{c.priceSignInRequired}</p>
            <button
              type="button"
              className="secondary-action"
              onClick={() => void refresh()}
              disabled={loading}
            >
              {c.retry}
            </button>
          </div>
        ) : accountModels.length === 0 ? (
          <p>{c.partialData}</p>
        ) : (
          <>
            <label className="account-model-picker">
              <span>{c.fullId}</span>
              <select
                value={selectedModelId}
                onChange={(event) => setSelectedModelId(event.target.value)}
              >
                {accountModels.map((model) => (
                  <option key={model.id} value={model.id}>
                    {model.id}
                  </option>
                ))}
              </select>
            </label>
            {selected && (
              <>
                <code className="selected-price-model">{selected.id}</code>
                {selected.description && (
                  <p className="selected-model-description">
                    {selected.description}
                  </p>
                )}
                {selected.pricingAvailable ? (
                  <div className="price-compare-grid">
                    <article>
                      <span>{c.officialPrice}</span>
                      <dl>
                        <div>
                          <dt>{c.inputPrice}</dt>
                          <dd>
                            {price(selected.officialInputCnyPerMillion)}
                            <small>{c.perMillionTokens}</small>
                          </dd>
                        </div>
                        <div>
                          <dt>{c.outputPrice}</dt>
                          <dd>
                            {price(selected.officialOutputCnyPerMillion)}
                            <small>{c.perMillionTokens}</small>
                          </dd>
                        </div>
                      </dl>
                    </article>
                    <article className="actual-price-card">
                      <span>{c.actualPrice}</span>
                      <dl>
                        <div>
                          <dt>{c.inputPrice}</dt>
                          <dd>
                            {price(selected.actualInputCnyPerMillion)}
                            <small>{c.perMillionTokens}</small>
                          </dd>
                        </div>
                        <div>
                          <dt>{c.outputPrice}</dt>
                          <dd>
                            {price(selected.actualOutputCnyPerMillion)}
                            <small>{c.perMillionTokens}</small>
                          </dd>
                        </div>
                      </dl>
                    </article>
                  </div>
                ) : (
                  <p className="account-inline-warning">
                    {c.priceUnavailable}
                  </p>
                )}
              </>
            )}
          </>
        )}

        <div className="fx-note">
          <span>{c.fx}</span>
          <strong>1 USD = 6.75 CNY</strong>
          <small>{c.fxNote}</small>
        </div>
        <p className="price-method-note">{c.priceMethodNote}</p>
      </section>
      <WorkbenchFooter />
    </div>
  );
}
