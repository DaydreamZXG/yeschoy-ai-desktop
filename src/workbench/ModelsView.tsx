import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDownUp, Layers3 } from "lucide-react";
import { ServiceCatalogPanel } from "../service-catalog/ServiceCatalogPanel";
import { COMPARISON_FX } from "../account/readiness";
import {
  CONFIGURATION_LINES,
  type ConfigurationLineId,
  type ConfigurationToolId,
} from "../configuration/preview";
import type { ToolAccessPlan } from "../service-catalog/access-plan";
import { useWorkbenchCopy } from "./copy";
import { WorkbenchFooter } from "./WorkbenchChrome";
const noReadSideEffect = () => {};
export function ModelsView() {
  const c = useWorkbenchCopy();
  const { t } = useTranslation();
  const [tool, setTool] = useState<ConfigurationToolId>("claude");
  const [line, setLine] = useState<ConfigurationLineId>("mainland_optimized");
  const [plan, setPlan] = useState<ToolAccessPlan | null>(null);
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
              onChange={(e) => setTool(e.target.value as ConfigurationToolId)}
            >
              <option value="claude">Claude Desktop</option>
              <option value="codex">Codex</option>
            </select>
          </label>
          <label>
            {c.selectLine}
            <select
              value={line}
              onChange={(e) => setLine(e.target.value as ConfigurationLineId)}
            >
              {CONFIGURATION_LINES.map((l) => (
                <option key={l.id} value={l.id}>
                  {t(`yeschoyConfiguration.lines.${l.id}.name`)}
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
      <section className="price-comparison-panel">
        <div className="workbench-section-heading">
          <h2>
            <ArrowDownUp aria-hidden="true" />
            {c.pricePending}
          </h2>
          <span className="status-badge">{c.priceUnavailable}</span>
        </div>
        <p>{c.pricePendingBody}</p>
        {plan && <code className="selected-price-model">{plan.modelId}</code>}
        <dl className="price-pair">
          <div>
            <dt>{c.officialPrice}</dt>
            <dd>—</dd>
          </div>
          <div>
            <dt>{c.actualPrice}</dt>
            <dd>—</dd>
          </div>
        </dl>
        <div className="fx-note">
          <span>{c.fx}</span>
          <strong>1 USD = {COMPARISON_FX.usdToCny} CNY</strong>
          <small>{c.fxNote}</small>
        </div>
      </section>
      <WorkbenchFooter />
    </div>
  );
}
