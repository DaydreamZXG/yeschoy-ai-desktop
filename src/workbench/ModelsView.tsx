import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDownUp, Layers3 } from "lucide-react";
import {
  BillingGroupPicker,
  BillingPrices,
} from "../configuration/BillingGroupPicker";
import { chooseBillingGroup } from "../configuration/billing";
import type { AccountModel } from "../account/session";
import type { AccountSessionController } from "../account/useAccountSession";
import {
  CONFIGURATION_LINES,
  type ConfigurationLineId,
} from "../configuration/preview";
import { type ActivationToolId } from "../configuration/activation";
import {
  modelSupportsTool,
  toolsSupportingModel,
} from "../configuration/modelCompatibility";
import { ModelPicker } from "../configuration/ModelPicker";
import { ModelCapabilityBadges } from "../model-profiles/ModelCapabilityBadges";
import { WORKBENCH_APPS } from "./appCatalog";
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
  const [tool, setTool] = useState<ActivationToolId>("claude_desktop");
  const [selectedModelId, setSelectedModelId] = useState("");
  const [billingGroup, setBillingGroup] = useState("");
  // #13 「查看全部模型」：默认只列兼容模型；打开后不兼容项置灰并标注原因。
  const [showAll, setShowAll] = useState(false);
  // 跟接入页说同一句话：不解释协议，直接点名哪个应用能用。
  const toolName =
    WORKBENCH_APPS.find((app) => app.id === tool)?.name ?? tool;
  const incompatibleReason = useCallback(
    (model: AccountModel) => {
      const endpoints = model.supportedEndpointTypes;
      if (modelSupportsTool(tool, endpoints)) return undefined;
      const elsewhere = toolsSupportingModel(endpoints, tool).map(
        (toolId) => WORKBENCH_APPS.find((app) => app.id === toolId)?.name ?? toolId,
      );
      return elsewhere.length
        ? c.incompatibleReason
            .replace("{{app}}", toolName)
            .replace("{{apps}}", elsewhere.join(c.appListSeparator))
        : c.incompatibleNowhere.replace("{{app}}", toolName);
    },
    [c, tool, toolName],
  );
  const { projection, loading, refresh } = session;
  const allModels = useMemo(
    () =>
      projection?.status === "signed_in" ? projection.models : EMPTY_MODELS,
    [projection],
  );
  const accountModels = useMemo(
    () =>
      showAll
        ? allModels
        : allModels.filter((model) =>
            modelSupportsTool(tool, model.supportedEndpointTypes),
          ),
    [allModels, showAll, tool],
  );

  // 选择始终落在兼容模型上：开关只影响可见范围，不允默认选不兼容项。
  const firstCompatibleId = accountModels.find((model) =>
    modelSupportsTool(tool, model.supportedEndpointTypes),
  )?.id;
  useEffect(() => {
    if (!accountModels.some((model) => model.id === selectedModelId))
      setSelectedModelId(firstCompatibleId ?? "");
  }, [accountModels, selectedModelId, firstCompatibleId]);

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
                setTool(event.target.value as ActivationToolId)
              }
            >
              {WORKBENCH_APPS.map((app) => (
                <option value={app.id} key={app.id}>
                  {app.name}
                </option>
              ))}
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
          {projection?.status === "signed_in" && allModels.length > 0 && (
            <label className="show-all-models-toggle">
              <input
                type="checkbox"
                checked={showAll}
                onChange={(event) => setShowAll(event.target.checked)}
              />
              {showAll ? c.showCompatibleOnly : c.showAllModels}
            </label>
          )}
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
        ) : allModels.length === 0 ? (
          // #13 空态区分：账户有数据但一个模型都没返回 → 数据未返回，
          // 与「无兼容模型」（见下方分支）不同口径。
          <p>{c.partialData}</p>
        ) : accountModels.length === 0 ? (
          <div className="price-account-empty">
            <p>{c.noCompatibleModels}</p>
            <button
              type="button"
              className="secondary-action"
              onClick={() => setShowAll(true)}
            >
              {c.showAllModels}
            </button>
          </div>
        ) : (
          <>
            <ModelPicker
              models={accountModels}
              value={selectedModelId}
              onChange={setSelectedModelId}
              label={c.fullId}
              disabledReason={showAll ? incompatibleReason : undefined}
            />
            {selected && (
              <>
                <code className="selected-price-model">{selected.id}</code>
                <ModelCapabilityBadges id={selected.id} />
                <BillingGroupPicker
                  model={selected}
                  fx={projection?.comparisonFx ?? ""}
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
              ? `参考换算值 ${projection.comparisonFx}`
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
