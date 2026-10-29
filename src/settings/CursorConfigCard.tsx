import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import catalog from "../model-profiles/catalog.json";
import { modelDisplayName } from "../model-profiles/profile";

/**
 * Cursor：命令行版可接入（写 `~/.cursor/cli-config.json`），桌面版不能。
 *
 * 桌面版的自定义端点存在 Cursor 自己的数据库里、密钥在它的钥匙串条目里，
 * 还要在 Cursor 界面里勾选启用。替用户去改那两处等于代它登录，出错会弄坏
 * Cursor 的登录态，所以这里只给出准确的说明，不做一键接入。
 */
const REASON_CODES = [
  "configured",
  "invalid_endpoint",
  "configuration_read_failed",
  "configuration_parse_failed",
  "configuration_write_failed",
  "configuration_readback_failed",
  "configuration_rollback_failed",
  "external_override",
  "model_required",
  "user_home_unavailable",
] as const;

export interface CursorIdeState {
  observed: boolean;
  customEndpointConfigured: boolean;
  existingProvider: string;
}

export interface CursorConfigSnapshot {
  toolId: string;
  cliDetected: boolean;
  endpoint: string;
  customEndpointConfigured: boolean;
  headerNames: string[];
  ide: CursorIdeState;
}

export interface CursorApplyResult {
  toolId: string;
  status: string;
  reasonCode: string;
  modelId: string | null;
}

function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function exactKeys(value: Record<string, unknown>, keys: readonly string[]) {
  const actual = Object.keys(value);
  return (
    actual.length === keys.length && keys.every((key) => actual.includes(key))
  );
}

const TOOL_ID = "cursor_agent";

export function decodeCursorSnapshot(
  value: unknown,
): CursorConfigSnapshot | null {
  if (
    !record(value) ||
    !exactKeys(value, [
      "toolId",
      "cliDetected",
      "endpoint",
      "customEndpointConfigured",
      "headerNames",
      "ide",
    ]) ||
    value.toolId !== TOOL_ID ||
    typeof value.cliDetected !== "boolean" ||
    typeof value.endpoint !== "string" ||
    value.endpoint.length > 300 ||
    typeof value.customEndpointConfigured !== "boolean" ||
    !Array.isArray(value.headerNames) ||
    value.headerNames.length > 32 ||
    value.headerNames.some(
      (name) => typeof name !== "string" || name.length > 80,
    ) ||
    !record(value.ide) ||
    !exactKeys(value.ide, [
      "observed",
      "customEndpointConfigured",
      "existingProvider",
    ]) ||
    typeof value.ide.observed !== "boolean" ||
    typeof value.ide.customEndpointConfigured !== "boolean" ||
    typeof value.ide.existingProvider !== "string"
  )
    return null;
  return value as unknown as CursorConfigSnapshot;
}

export function decodeApplyResult(value: unknown): CursorApplyResult | null {
  if (
    !record(value) ||
    !exactKeys(value, ["toolId", "status", "reasonCode", "modelId"]) ||
    value.toolId !== TOOL_ID ||
    typeof value.status !== "string" ||
    !(REASON_CODES as readonly string[]).includes(value.reasonCode as string) ||
    !(value.modelId === null || typeof value.modelId === "string")
  )
    return null;
  return value as unknown as CursorApplyResult;
}

/**
 * 默认模型：优先取工具模型档案里的第一条（目录顺序即推荐顺序），
 * 不自己编一个型号。
 */
export function suggestedModel(): string | null {
  const first = catalog.models[0]?.id;
  return typeof first === "string" && first.length > 0 ? first : null;
}

type Phase = "loading" | "ready" | "applying" | "reverting" | "error";

export function CursorConfigCard() {
  const { t } = useTranslation();
  const [snapshot, setSnapshot] = useState<CursorConfigSnapshot | null>(null);
  const [phase, setPhase] = useState<Phase>("loading");
  const [outcome, setOutcome] = useState<CursorApplyResult | null>(null);
  const [confirming, setConfirming] = useState(false);
  const sequence = useRef(0);

  const load = useCallback(async () => {
    sequence.current += 1;
    const generation = sequence.current;
    try {
      const raw = await invoke<unknown>("cursor_config_read_only");
      if (generation !== sequence.current) return;
      const decoded = decodeCursorSnapshot(raw);
      if (!decoded) {
        setPhase("error");
        return;
      }
      setSnapshot(decoded);
      setPhase("ready");
    } catch {
      if (generation === sequence.current) setPhase("error");
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const apply = async () => {
    const model = suggestedModel();
    setConfirming(false);
    setOutcome(null);
    setPhase("applying");
    try {
      const raw = await invoke<unknown>("cursor_apply_v1", {
        request: { defaultModel: model },
      });
      const result = decodeApplyResult(raw);
      setOutcome(result ?? null);
      setPhase(result?.status === "applied" ? "ready" : "error");
      await load();
    } catch {
      setPhase("error");
    }
  };

  const revert = async () => {
    setOutcome(null);
    setPhase("reverting");
    try {
      const raw = await invoke<unknown>("cursor_revert_v1", { request: {} });
      setOutcome(decodeApplyResult(raw));
      await load();
      setPhase("ready");
    } catch {
      setPhase("error");
    }
  };

  const model = suggestedModel();
  const busy = phase === "applying" || phase === "reverting";

  return (
    <div className="cursor-card" data-testid="cursor-config" data-phase={phase}>
      <div className="setting-row-copy">
        <strong>{t("yeschoyCursor.title")}</strong>
        <p>{t("yeschoyCursor.body")}</p>
      </div>

      {outcome && (
        <div
          className={
            outcome.status === "applied" || outcome.status === "reverted"
              ? "residue-success"
              : "error-banner"
          }
          role={
            outcome.status === "applied" || outcome.status === "reverted"
              ? "status"
              : "alert"
          }
          data-testid="cursor-outcome"
        >
          <strong>
            {t(`yeschoyCursor.outcomes.${outcome.reasonCode}.title`)}
          </strong>
          <span>{t(`yeschoyCursor.outcomes.${outcome.reasonCode}.body`)}</span>
        </div>
      )}

      {snapshot && (
        <>
          <div className="cursor-row" data-testid="cursor-cli">
            <div>
              <strong>{t("yeschoyCursor.cli.title")}</strong>
              <p>{t("yeschoyCursor.cli.body")}</p>
            </div>
            <span
              className="cursor-status"
              data-detected={snapshot.cliDetected}
            >
              {snapshot.cliDetected
                ? t("yeschoyCursor.cli.detected")
                : t("yeschoyCursor.cli.notDetected")}
            </span>
          </div>

          {snapshot.cliDetected && (
            <>
              <dl className="cursor-facts">
                <div>
                  <dt>{t("yeschoyCursor.facts.endpoint")}</dt>
                  <dd>{snapshot.endpoint}</dd>
                </div>
                <div>
                  <dt>{t("yeschoyCursor.facts.model")}</dt>
                  <dd data-testid="cursor-model">
                    {model ? modelDisplayName(model) : "—"}
                  </dd>
                </div>
                <div>
                  <dt>{t("yeschoyCursor.facts.state")}</dt>
                  <dd>
                    {snapshot.customEndpointConfigured
                      ? t("yeschoyCursor.facts.configured")
                      : t("yeschoyCursor.facts.notConfigured")}
                  </dd>
                </div>
              </dl>

              <div className="residue-actions">
                {confirming ? (
                  <>
                    <p className="residue-warning">
                      {t("yeschoyCursor.confirmBody")}
                    </p>
                    <button
                      type="button"
                      className="primary-action"
                      onClick={() => void apply()}
                      data-testid="cursor-confirm"
                    >
                      {t("yeschoyCursor.confirmAction")}
                    </button>
                    <button type="button" onClick={() => setConfirming(false)}>
                      {t("yeschoyCursor.cancel")}
                    </button>
                  </>
                ) : (
                  <>
                    <button
                      type="button"
                      disabled={busy || !model}
                      onClick={() => setConfirming(true)}
                      data-testid="cursor-apply"
                    >
                      {snapshot.customEndpointConfigured
                        ? t("yeschoyCursor.reapplyAction")
                        : t("yeschoyCursor.applyAction")}
                    </button>
                    {snapshot.customEndpointConfigured && (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() => void revert()}
                        data-testid="cursor-revert"
                      >
                        {t("yeschoyCursor.revertAction")}
                      </button>
                    )}
                  </>
                )}
              </div>
              <p className="diagnostic-consent">{t("yeschoyCursor.howTo")}</p>
              <p className="diagnostic-consent">{t("yeschoyCursor.keyNote")}</p>
            </>
          )}

          <div className="cursor-row" data-testid="cursor-ide">
            <div>
              <strong>{t("yeschoyCursor.ide.title")}</strong>
              <p>{t("yeschoyCursor.ide.body")}</p>
            </div>
            <span
              className="cursor-status"
              data-detected={snapshot.ide.observed}
            >
              {snapshot.ide.observed
                ? t("yeschoyCursor.ide.observed")
                : t("yeschoyCursor.ide.notObserved")}
            </span>
          </div>
          <ol className="cursor-steps" data-testid="cursor-ide-steps">
            <li>{t("yeschoyCursor.ide.step1")}</li>
            <li>{t("yeschoyCursor.ide.step2")}</li>
            <li>{t("yeschoyCursor.ide.step3")}</li>
          </ol>
          <p className="residue-warning">{t("yeschoyCursor.ide.warning")}</p>
        </>
      )}

      {!snapshot && phase === "loading" && (
        <p className="diagnostic-consent" role="status">
          {t("yeschoyCursor.loading")}
        </p>
      )}
      {!snapshot && phase === "error" && !outcome && (
        <div className="error-banner" role="alert">
          <strong>{t("yeschoyCursor.readErrorTitle")}</strong>
          <span>{t("yeschoyCursor.readErrorBody")}</span>
        </div>
      )}
    </div>
  );
}
