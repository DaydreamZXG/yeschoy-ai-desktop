import { useEffect, useMemo, useState } from "react";
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
} from "../configuration/preview";
import { type ActivationToolId } from "../configuration/activation";
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
  const { projection, loading, refresh } = session;
  const allModels = useMemo(
    () =>
      projection?.status === "signed_in" ? projection.models : EMPTY_MODELS,
    [projection],
  );
  // 不再按协议过滤或置灰，见 modelCompatibility.ts 顶部。列表就是账户的
  // 全部模型，「查看全部模型」那个开关也跟着没了 —— 已经没有「不全」的状态。
  const accountModels = allModels;
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
        ) : (
          <>
            <ModelPicker
              models={accountModels}
              value={selectedModelId}
              onChange={setSelectedModelId}
              label={c.fullId}
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
