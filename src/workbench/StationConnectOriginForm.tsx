import { useCallback, useEffect, useRef, useState } from "react";
import {
  ArrowRight,
  CheckCircle2,
  CircleAlert,
  KeyRound,
  LoaderCircle,
  ShieldQuestion,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  openStationConnectPage,
  readStationOriginDraftInitial,
  STATION_ORIGIN_DRAFT_KEY,
} from "../account/openStationConnectPage";
import { stationHost } from "../account/StationTrustDialog";
import { canonicalStationHttpsOrigin } from "../../connect/stationConnectPage";
import type { AccountLineId } from "../configuration/preview";
import {
  BUILTIN_STATION_ORIGINS,
  probeStation,
  type StationProbeResult,
} from "../station/stationProbe";
import { modelDisplayName, shortModelId } from "../model-profiles/profile";

interface Props {
  onLineChange: (lineId: AccountLineId) => void;
  disabled?: boolean;
}

/** 小白常直接粘贴后台页面地址或漏掉 https://，这里替他收拾成站点根地址。 */
export function normalizeStationInput(raw: string): string | null {
  const text = raw.trim();
  if (!text) return null;
  const withScheme = /^[a-z]+:\/\//i.test(text) ? text : `https://${text}`;
  try {
    const url = new URL(withScheme);
    return canonicalStationHttpsOrigin(url.origin);
  } catch {
    return null;
  }
}

type Stage =
  | { kind: "idle" }
  | { kind: "probing" }
  | { kind: "result"; probe: StationProbeResult }
  | { kind: "failed"; reason: string };

/** 前端再加一道软时限：原生侧挂住时，界面也要能取消、能重试。 */
const PROBE_UI_DEADLINE_MS = 4_500;

async function probeWithDeadline(
  origin: string,
  token?: string,
): Promise<StationProbeResult | null | "timeout"> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      probeStation(origin, token),
      new Promise<"timeout">((resolve) => {
        timer = setTimeout(() => resolve("timeout"), PROBE_UI_DEADLINE_MS);
      }),
    ]);
  } finally {
    if (timer) clearTimeout(timer);
  }
}

/**
 * 「接入你的中转站」：
 * 1. 粘贴网址 → 助手自己检测站点类型；
 * 2. 站点支持一键登录 → 确认后去浏览器登录；
 * 3. 不支持 → 改用令牌：贴一个 sk- 开头的令牌，助手验证并列出可用模型。
 * 站点那边不需要做任何改动。检测不挡其它页面操作；可取消、可重试。
 */
export function StationConnectOriginForm({ onLineChange, disabled }: Props) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState(readStationOriginDraftInitial);
  const [error, setError] = useState<string | null>(null);
  const [opening, setOpening] = useState(false);
  const [stage, setStage] = useState<Stage>({ kind: "idle" });
  const [token, setToken] = useState("");
  const [checkingToken, setCheckingToken] = useState(false);
  const probeGeneration = useRef(0);
  const hintId = "station-origin-hint";
  const describedBy = error ? `${hintId} station-origin-error` : hintId;

  useEffect(
    () => () => {
      // 离开页面时作废进行中的检测，避免回来时突然弹出结果。
      probeGeneration.current += 1;
    },
    [],
  );

  const cancelProbe = useCallback(() => {
    probeGeneration.current += 1;
    setStage({ kind: "idle" });
    setCheckingToken(false);
  }, []);

  const openLogin = useCallback(
    async (origin: string) => {
      setOpening(true);
      const result = await openStationConnectPage(origin);
      setOpening(false);
      if (!result.ok) {
        setError(
          result.reason === "browser_open_failed"
            ? t("stationConnect.reason.connect_browser_open_failed")
            : t("stationConnectPage.invalidOrigin"),
        );
        return;
      }
      try {
        localStorage.setItem(STATION_ORIGIN_DRAFT_KEY, result.origin);
      } catch {
        /* ignore */
      }
      onLineChange(result.lineId);
    },
    [onLineChange, t],
  );

  const submit = useCallback(async () => {
    setError(null);
    const origin = normalizeStationInput(draft);
    if (!origin) {
      setError(t("stationConnectPage.invalidOrigin"));
      return;
    }
    if (BUILTIN_STATION_ORIGINS.includes(origin)) {
      await openLogin(origin);
      return;
    }
    const generation = ++probeGeneration.current;
    setStage({ kind: "probing" });
    const probe = await probeWithDeadline(origin);
    if (generation !== probeGeneration.current) return;
    if (probe === "timeout") {
      setStage({ kind: "failed", reason: "timeout" });
      return;
    }
    if (!probe) {
      setStage({ kind: "failed", reason: "unreachable" });
      return;
    }
    if (probe.status !== "ok") {
      setStage({ kind: "failed", reason: probe.status });
      return;
    }
    setStage({ kind: "result", probe });
  }, [draft, openLogin, t]);

  const checkToken = useCallback(async () => {
    if (stage.kind !== "result") return;
    const generation = ++probeGeneration.current;
    setCheckingToken(true);
    const probe = await probeWithDeadline(stage.probe.origin, token.trim());
    if (generation !== probeGeneration.current) return;
    setCheckingToken(false);
    if (probe === "timeout" || !probe) {
      setStage({
        kind: "failed",
        reason: probe === "timeout" ? "timeout" : "unreachable",
      });
      return;
    }
    // 令牌验证完就从输入框清掉，不在界面上长时间露着。
    if (probe.tokenStatus === "valid") setToken("");
    setStage({ kind: "result", probe });
  }, [stage, token]);

  const busy = disabled || opening || stage.kind === "probing";
  const probe = stage.kind === "result" ? stage.probe : null;

  return (
    <div className="station-onboarding">
      <form
        className="station-connect-origin-form"
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
      >
        <label className="station-connect-origin-label">
          <span>{t("stationConnectPage.originLabel")}</span>
          {/* 不用 type="url"：浏览器会拒绝没写 https:// 的输入，而小白最常这么贴。 */}
          <input
            type="text"
            inputMode="url"
            autoComplete="url"
            spellCheck={false}
            placeholder={t("stationConnectPage.originPlaceholder")}
            value={draft}
            disabled={disabled || opening}
            onChange={(event) => {
              setDraft(event.target.value);
              if (error) setError(null);
              if (stage.kind === "failed" || stage.kind === "result")
                setStage({ kind: "idle" });
            }}
            aria-invalid={error ? true : undefined}
            aria-describedby={describedBy}
          />
        </label>
        {error && (
          <p
            className="station-connect-origin-error"
            id="station-origin-error"
            role="alert"
          >
            {error}
          </p>
        )}
        <div className="station-connect-origin-actions">
          <button
            type="submit"
            className="primary-action station-connect-origin-submit"
            disabled={busy || !draft.trim()}
          >
            {stage.kind === "probing" ? (
              <>
                <LoaderCircle className="is-spinning" aria-hidden="true" />
                {t("stationOnboarding.probing")}
              </>
            ) : opening ? (
              t("stationConnectPage.opening")
            ) : (
              <>
                {t("stationConnectPage.continue")}
                <ArrowRight aria-hidden="true" />
              </>
            )}
          </button>
          {stage.kind === "probing" && (
            <button type="button" className="text-button" onClick={cancelProbe}>
              <X aria-hidden="true" />
              {t("stationOnboarding.cancelProbe")}
            </button>
          )}
        </div>
        <small className="station-connect-origin-hint" id={hintId}>
          {t("stationConnectPage.originHint")}
        </small>
      </form>

      {stage.kind === "probing" && (
        <div
          className="station-onboarding-card"
          data-tone="progress"
          role="status"
          data-testid="station-probing"
        >
          <LoaderCircle className="is-spinning" aria-hidden="true" />
          <div>
            <strong>{t("stationOnboarding.probingTitle")}</strong>
            <p>{t("stationOnboarding.probingBody")}</p>
          </div>
        </div>
      )}

      {stage.kind === "failed" && (
        <div
          className="station-onboarding-card"
          data-tone="danger"
          role="alert"
          data-testid="station-probe-failed"
        >
          <CircleAlert aria-hidden="true" />
          <div>
            <strong>
              {t(`stationOnboarding.failed.${stage.reason}.title`)}
            </strong>
            <p>{t(`stationOnboarding.failed.${stage.reason}.next`)}</p>
            <button
              type="button"
              className="subtle-button"
              disabled={disabled}
              onClick={() => void submit()}
            >
              {t("stationOnboarding.retryProbe")}
            </button>
          </div>
        </div>
      )}

      {probe && (
        <div className="station-onboarding-card" data-tone="info">
          <ShieldQuestion aria-hidden="true" />
          <div>
            <strong>
              {t("stationOnboarding.found", {
                host: stationHost(probe.origin),
              })}
            </strong>
            <p className="station-onboarding-meta">
              {[
                probe.systemName,
                t(`stationOnboarding.kind.${probe.kind}`),
                probe.version,
              ]
                .filter(Boolean)
                .join(" · ")}
            </p>

            {probe.desktopConnect ? (
              <>
                <p>{t("stationOnboarding.oneClickBody")}</p>
                <p className="station-onboarding-warn">
                  {t("stationOnboarding.trustQuestion")}
                </p>
                <button
                  type="button"
                  className="primary-action"
                  disabled={busy}
                  onClick={() => void openLogin(probe.origin)}
                >
                  {t("stationOnboarding.trustAndLogin")}
                  <ArrowRight aria-hidden="true" />
                </button>
              </>
            ) : (
              <>
                <p>{t("stationOnboarding.tokenIntro")}</p>
                <ol className="station-onboarding-steps">
                  <li>{t("stationOnboarding.tokenStep1")}</li>
                  <li>{t("stationOnboarding.tokenStep2")}</li>
                  <li>{t("stationOnboarding.tokenStep3")}</li>
                </ol>
                <form
                  className="station-token-form"
                  onSubmit={(event) => {
                    event.preventDefault();
                    void checkToken();
                  }}
                >
                  <label>
                    <span>
                      <KeyRound aria-hidden="true" />
                      {t("stationOnboarding.tokenLabel")}
                    </span>
                    <input
                      type="password"
                      autoComplete="off"
                      spellCheck={false}
                      placeholder="sk-..."
                      value={token}
                      onChange={(event) => setToken(event.target.value)}
                    />
                  </label>
                  <button
                    type="submit"
                    className="subtle-button"
                    disabled={checkingToken || !token.trim()}
                  >
                    {checkingToken
                      ? t("stationOnboarding.tokenChecking")
                      : t("stationOnboarding.tokenCheck")}
                  </button>
                </form>
                {probe.tokenStatus !== "not_provided" && (
                  <p
                    className="station-token-result"
                    data-ok={probe.tokenStatus === "valid"}
                    role="status"
                  >
                    {probe.tokenStatus === "valid" ? (
                      <CheckCircle2 aria-hidden="true" />
                    ) : (
                      <CircleAlert aria-hidden="true" />
                    )}
                    {t(`stationOnboarding.token.${probe.tokenStatus}`, {
                      count: probe.models.length,
                    })}
                  </p>
                )}
                {probe.tokenStatus === "valid" && probe.models.length > 0 && (
                  <ul
                    className="station-model-chips"
                    aria-label={t("stationOnboarding.modelsLabel")}
                  >
                    {probe.models.slice(0, 24).map((model) => (
                      <li key={model} title={model}>
                        <strong>{modelDisplayName(model)}</strong>
                        {modelDisplayName(model) !== model && (
                          <code>{shortModelId(model, 18)}</code>
                        )}
                      </li>
                    ))}
                    {probe.models.length > 24 && (
                      <li className="more">
                        {t("stationOnboarding.moreModels", {
                          count: probe.models.length - 24,
                        })}
                      </li>
                    )}
                  </ul>
                )}
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
