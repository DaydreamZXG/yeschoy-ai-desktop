import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";

/**
 * 「0 垃圾留存」：卸载前列出助手自己留下的东西，并在确认后删除。
 *
 * 原则和接入一样：盘点是只读的，删除要显式勾选 + 二次确认；原生侧只删
 * 白名单位置，钥匙串只动本应用自己的条目。用户在应用里已接入的应用
 * 配置不在这里处理 —— 那要走「恢复原设置」。
 */
export const RESIDUE_STATES = ["ready", "action", "kept"] as const;
export type ResidueState = (typeof RESIDUE_STATES)[number];

export interface ResidueItem {
  id: string;
  target: string;
  state: ResidueState;
  reasonCode: string;
  selectedByDefault: boolean;
  entries: string[];
}

export interface ResidueInventory {
  requestId: string;
  clean: boolean;
  items: ResidueItem[];
}

const ITEM_STATES: readonly string[] = RESIDUE_STATES;
const ITEM_TARGETS = [
  "state_dir",
  "log_dir",
  "webview_data",
  "keychain",
  "tool_configs",
  "os_crash_reports",
] as const;

function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function exactKeys(value: Record<string, unknown>, keys: readonly string[]) {
  const actual = Object.keys(value);
  return (
    actual.length === keys.length && keys.every((key) => actual.includes(key))
  );
}

function safeName(value: unknown, max: number): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= max;
}

export function decodeResidueInventory(
  value: unknown,
  requestId: string,
): ResidueInventory | null {
  if (
    !record(value) ||
    !exactKeys(value, ["requestId", "clean", "items"]) ||
    value.requestId !== requestId ||
    typeof value.clean !== "boolean" ||
    !Array.isArray(value.items) ||
    value.items.length > 32
  )
    return null;
  const items: ResidueItem[] = [];
  for (const item of value.items) {
    if (
      !record(item) ||
      !exactKeys(item, [
        "id",
        "target",
        "state",
        "reasonCode",
        "selectedByDefault",
        "entries",
      ]) ||
      !safeName(item.id, 40) ||
      !(ITEM_TARGETS as readonly unknown[]).includes(item.target) ||
      !ITEM_STATES.includes(item.state as string) ||
      !safeName(item.reasonCode, 60) ||
      typeof item.selectedByDefault !== "boolean" ||
      !Array.isArray(item.entries) ||
      item.entries.length > 200 ||
      item.entries.some(
        (entry) => typeof entry !== "string" || entry.length > 200,
      )
    )
      return null;
    items.push(item as unknown as ResidueItem);
  }
  if (new Set(items.map((item) => item.id)).size !== items.length) return null;
  return { requestId, clean: value.clean, items };
}

type Phase = "loading" | "ready" | "clearing" | "cleared" | "error";

export function ResidueCard() {
  const { t } = useTranslation();
  const [inventory, setInventory] = useState<ResidueInventory | null>(null);
  const [phase, setPhase] = useState<Phase>("loading");
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());
  const [confirming, setConfirming] = useState(false);
  const [outcome, setOutcome] = useState<"cleared" | "partial" | null>(null);
  const sequence = useRef(0);
  const latestRequest = useRef("");

  const load = useCallback(async () => {
    sequence.current += 1;
    const requestId = `residue-${Date.now().toString(36)}-${sequence.current}`;
    latestRequest.current = requestId;
    setPhase("loading");
    try {
      const raw = await invoke<unknown>("residue_inventory_read_only", {
        requestId,
      });
      // 只认最后一次盘点：清理后会紧接着重新盘点，旧回复晚到不能覆盖新结果。
      if (requestId !== latestRequest.current) return;
      const decoded = decodeResidueInventory(raw, requestId);
      if (!decoded) {
        setPhase("error");
        return;
      }
      setInventory(decoded);
      // 必选项默认勾上；需要用户先做一步的（待恢复的接入）不预选。
      setSelected(
        new Set(
          decoded.items
            .filter((item) => item.state === "ready" && item.selectedByDefault)
            .map((item) => item.id),
        ),
      );
      setPhase("ready");
    } catch {
      if (requestId === latestRequest.current) setPhase("error");
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const toggle = (id: string) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const clear = async () => {
    setConfirming(false);
    setPhase("clearing");
    setOutcome(null);
    const requestId = `residue-clear-${Date.now().toString(36)}`;
    try {
      await invoke("residue_clear_v1", {
        requestId,
        includeState: selected.has("state"),
        includeLogs: selected.has("logs"),
        includeWebviewData: [...selected].some(
          (id) => id !== "state" && id !== "logs" && id !== "keychain",
        ),
        includeKeychain: selected.has("keychain"),
      });
      setOutcome("cleared");
      await load();
      setPhase("cleared");
    } catch {
      setOutcome("partial");
      setPhase("error");
    }
  };

  const removable = inventory?.items.filter(
    (item) => item.state === "ready" && selected.has(item.id),
  );
  const actionable =
    inventory?.items.filter((item) => item.state === "action") ?? [];
  const kept = inventory?.items.filter((item) => item.state === "kept");

  return (
    <div className="residue-card" data-testid="residue-card" data-phase={phase}>
      <div className="setting-row-copy">
        <strong>{t("yeschoyResidue.title")}</strong>
        <p>{t("yeschoyResidue.body")}</p>
      </div>

      {phase === "loading" && inventory === null && (
        <p className="diagnostic-consent" role="status">
          {t("yeschoyResidue.scanning")}
        </p>
      )}
      {phase === "error" && (
        <div className="error-banner" role="alert">
          <strong>
            {outcome === "partial"
              ? t("yeschoyResidue.partialTitle")
              : t("yeschoyResidue.errorTitle")}
          </strong>
          <span>
            {outcome === "partial"
              ? t("yeschoyResidue.partialBody")
              : t("yeschoyResidue.errorBody")}
          </span>
        </div>
      )}

      {inventory && (
        <>
          {phase === "cleared" && (
            <p
              className="residue-success"
              role="status"
              data-testid="residue-cleared"
            >
              {inventory.clean
                ? t("yeschoyResidue.clearedClean")
                : t("yeschoyResidue.clearedRemaining")}
            </p>
          )}
          {inventory.clean && phase !== "cleared" && (
            <p className="line-waiting-copy" data-testid="residue-clean">
              {t("yeschoyResidue.nothingLeft")}
            </p>
          )}

          {inventory.items.length > 0 && (
            <ul className="residue-items" data-testid="residue-items">
              {inventory.items.map((item) => (
                <li
                  className="residue-item"
                  data-state={item.state}
                  key={item.id}
                >
                  {item.state === "ready" ? (
                    <label className="residue-item-label">
                      <input
                        type="checkbox"
                        checked={selected.has(item.id)}
                        onChange={() => toggle(item.id)}
                      />
                      <span>
                        <strong>
                          {t(`yeschoyResidue.items.${item.id}.title`)}
                        </strong>
                        <p>{t(`yeschoyResidue.items.${item.id}.body`)}</p>
                      </span>
                    </label>
                  ) : (
                    <span className="residue-item-static">
                      <strong>
                        {t(`yeschoyResidue.items.${item.id}.title`)}
                      </strong>
                      <p>
                        {t(`yeschoyResidue.reasons.${item.reasonCode}.note`)}
                        {item.state === "action" &&
                          ` ${t(`yeschoyResidue.reasons.${item.reasonCode}.instruction`)}`}
                      </p>
                    </span>
                  )}
                  {item.entries.length > 0 && (
                    <details className="residue-preview">
                      <summary>
                        {t("yeschoyResidue.previewTitle", {
                          count: item.entries.length,
                        })}
                      </summary>
                      <ul>
                        {item.entries.map((entry) => (
                          <li key={entry}>{entry}</li>
                        ))}
                      </ul>
                    </details>
                  )}
                </li>
              ))}
            </ul>
          )}

          {actionable.length > 0 && (
            <p className="residue-warning" data-testid="residue-restore-first">
              {t("yeschoyResidue.restoreFirst", { count: actionable.length })}
            </p>
          )}

          {removable && removable.length > 0 && (
            <div className="residue-actions">
              {confirming ? (
                <>
                  <p className="residue-warning">
                    {t("yeschoyResidue.confirmBody")}
                  </p>
                  <button
                    type="button"
                    className="primary-action"
                    onClick={() => void clear()}
                    data-testid="residue-confirm"
                  >
                    {t("yeschoyResidue.confirmAction")}
                  </button>
                  <button type="button" onClick={() => setConfirming(false)}>
                    {t("yeschoyResidue.cancel")}
                  </button>
                </>
              ) : (
                <button
                  type="button"
                  onClick={() => setConfirming(true)}
                  data-testid="residue-clear"
                >
                  {t("yeschoyResidue.clearAction", { count: removable.length })}
                </button>
              )}
            </div>
          )}

          {kept && kept.length > 0 && (
            <p className="diagnostic-consent">
              {t("yeschoyResidue.keptNote", { count: kept.length })}
            </p>
          )}
        </>
      )}

      <p className="diagnostic-consent">{t("yeschoyResidue.scopeNote")}</p>
    </div>
  );
}
