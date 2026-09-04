import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDownUp, Layers3 } from "lucide-react";
import {
  BillingGroupPicker,
  BillingPrices,
} from "../configuration/BillingGroupPicker";
import { chooseBillingGroup } from "../configuration/billing";
import type { AccountSessionController } from "../account/useAccountSession";
import {
  CONFIGURATION_LINES,
  type ConfigurationLineId,
  type ConfigurationToolId,
} from "../configuration/preview";
import { useWorkbenchCopy } from "./copy";
import { WorkbenchFooter } from "./WorkbenchChrome";

const EMPTY_MODELS: never[] = [];

export function ModelsView({
  line,
  onLineChange,
  session,
  onOpenAccount,
}: {
  line: ConfigurationLineId;
  onLineChange: (line: ConfigurationLineId) => void;
  session: AccountSessionController;
  onOpenAccount: () => void;
}) {
  const c = useWorkbenchCopy();
  const { t } = useTranslation();
  const [tool, setTool] = useState<ConfigurationToolId>("claude");
  const [selectedModelId, setSelectedModelId] = useState("");
  const [billingGroup, setBillingGroup] = useState("");
  const { projection, loading, refresh } = session;
  const accountModels =
    projection?.status === "signed_in" ? projection.models : EMPTY_MODELS;

  useEffect(() => {
    if (!accountModels.some((model) => model.id === selectedModelId))
      setSelectedModelId(accountModels[0]?.id ?? "");
  }, [accountModels, selectedModelId]);

  const selected = accountModels.find((model) => model.id === selectedModelId);
  useEffect(
    () => setBillingGroup((previous) => chooseBillingGroup(selected, previous)),
    [selected],
  );

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
                onLineChange(event.target.value as ConfigurationLineId)
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
        <div className="account-model-catalog-status" role="status">
          <div>
            <strong>
              {projection?.status === "signed_in"
                ? c.availableModels.replace(
                    "{{count}}",
                    String(accountModels.length),
                  )
                : c.priceSignInRequired}
            </strong>
            <small>{c.fullId}</small>
          </div>
          <button
            type="button"
            className="secondary-action"
            onClick={() =>
              projection?.status === "signed_in"
                ? void refresh()
                : onOpenAccount()
            }
            disabled={loading}
          >
            {projection?.status === "signed_in" ? c.refresh : c.signIn}
          </button>
        </div>
      </section>

      <section className="price-comparison-panel account-price-panel">
        <div className="workbench-section-heading">
          <h2>
            <ArrowDownUp aria-hidden="true" />
            {c.officialPrice} / {c.actualPrice}
          </h2>
        </div>

        {projection?.status !== "signed_in" ? (
          <div className="price-account-empty">
            <p>{c.priceSignInRequired}</p>
            <button
              type="button"
              className="secondary-action"
              onClick={onOpenAccount}
              disabled={loading}
            >
              {c.signIn}
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
                <BillingGroupPicker
                  model={selected}
                  selected={billingGroup}
                  onChange={setBillingGroup}
                />
                <BillingPrices
                  model={selected}
                  selected={billingGroup}
                  fx={projection?.comparisonFx ?? ""}
                />
              </>
            )}
          </>
        )}

        <div className="fx-note">
          <span>{c.fx}</span>
          <strong>
            {projection?.comparisonFx
              ? `1 USD = ${projection.comparisonFx} CNY`
              : "—"}
          </strong>
          <small>{c.serverFxNote}</small>
        </div>
        <p className="price-method-note">{c.priceMethodNote}</p>
      </section>
      <WorkbenchFooter />
    </div>
  );
}
