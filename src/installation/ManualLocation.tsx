import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { FolderSearch, RotateCcw } from "lucide-react";
import type {
  ActivationToolId,
  ManualLocationState,
} from "../configuration/activation";
import {
  clearManualLocation,
  pickManualLocation,
  type ManualLocationOutcome,
} from "./manualLocationApi";

/**
 * 自动检测找不到时（免安装版、换过目录、安装程序没登记、命令行工具
 * 不在 PATH 里），让用户自己指一下。选完立即重新检查。
 */
export function useManualLocation(
  tool: ActivationToolId,
  onChanged: () => void,
) {
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<ManualLocationOutcome | null>(null);
  const run = useCallback(
    async (action: typeof pickManualLocation) => {
      setBusy(true);
      setOutcome(null);
      const result = await action(tool);
      setBusy(false);
      setOutcome(result);
      if (result === "saved" || result === "cleared") onChanged();
    },
    [tool, onChanged],
  );
  return {
    busy,
    outcome,
    pick: () => void run(pickManualLocation),
    clear: () => void run(clearManualLocation),
  };
}

export function ManualLocationMessage({
  outcome,
  name,
}: {
  outcome: ManualLocationOutcome | null;
  name: string;
}) {
  const { t } = useTranslation();
  if (!outcome || outcome === "cancelled") return null;
  const failed = !["saved", "cleared"].includes(outcome);
  return (
    <p
      className={failed ? "app-install-warning" : "app-install-copy"}
      role={failed ? "alert" : "status"}
    >
      {t(`manualLocation.outcome.${outcome}`, { name })}
    </p>
  );
}

/** 安装详情里的手动位置：说明当前用的是哪个，并能改回自动检测。 */
export function ManualLocationControls({
  tool,
  name,
  state,
  onChanged,
  disabled,
}: {
  tool: ActivationToolId;
  name: string;
  state: ManualLocationState;
  onChanged: () => void;
  disabled?: boolean;
}) {
  const { t } = useTranslation();
  const { busy, outcome, pick, clear } = useManualLocation(tool, onChanged);
  return (
    <div className="manual-location" data-state={state}>
      {state === "in_use" && (
        <p className="app-install-copy">{t("manualLocation.inUse")}</p>
      )}
      {state === "unavailable" && (
        <p className="app-install-warning">
          {t("manualLocation.unavailable", { name })}
        </p>
      )}
      <div className="manual-location-actions">
        <button
          type="button"
          className="text-button"
          disabled={busy || disabled}
          onClick={pick}
        >
          <FolderSearch size={13} aria-hidden="true" />
          {state === "none"
            ? t("manualLocation.pickOther")
            : t("manualLocation.pickAgain")}
        </button>
        {state !== "none" && (
          <button
            type="button"
            className="text-button"
            disabled={busy || disabled}
            onClick={clear}
          >
            <RotateCcw size={13} aria-hidden="true" />
            {t("manualLocation.reset")}
          </button>
        )}
      </div>
      <ManualLocationMessage outcome={outcome} name={name} />
    </div>
  );
}
