import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type {
  ConnectionResponse,
  ToolConnection,
} from "../configuration/connections";
import { DESKTOP_DOWNLOAD_PAGE_URL } from "../distribution/desktopDownload";
import {
  LANGUAGE_NAME,
  SUPPORTED_LANGUAGES,
  announceLanguageToNative,
  applyHtmlLang,
  storeLanguage,
  type Language,
} from "../i18n/language";
import { useDesktopUpdate } from "../update/UpdateProvider";
import { reportText, type CheckReport } from "./checkup";
import { Drawer } from "./Drawer";
import type { Mood, RosterId } from "./roster";
import { isStationKeyLineId } from "../configuration/preview";
import { askToQuit } from "./Shutdown";
import { isMuted, setMuted, sound } from "./sound";
import { undoKey } from "./undo";

type ToolId = ToolConnection["toolId"];

/**
 * 改回原样之前问的那句话。粘贴密钥接的应用用的是用户自己的那把密钥，
 * 客户端不会、也不该替他去站点作废，所以不说「停用密钥」。
 */
const askKey = (connection: ToolConnection | undefined) =>
  `kaifan.settings.ask${
    connection?.restoreMode === "original" ? "Restore" : "Remove"
  }${connection && isStationKeyLineId(connection.lineId) ? "OwnKey" : ""}`;

export interface Checkup {
  running: boolean;
  report: CheckReport | null;
  run: () => void;
}

interface Props {
  open: boolean;
  who: RosterId;
  /** 站点现在的情况：连着、没连、连不上、正在连。 */
  reach: "ok" | "out" | "off" | "wait";
  /** 连的是哪个站；内置线路没有地址可说。 */
  host: string | null;
  /** 是粘贴密钥连的，不是在浏览器里登录的。 */
  viaKey: boolean;
  connections: ToolConnection[];
  /** 正在改回原样的那个应用。 */
  restoring: ToolId | null;
  appName: (toolId: string) => string;
  checkup: Checkup;
  onConnect: () => void;
  /** 退出登录；成了回 true。 */
  onLogout: () => Promise<boolean>;
  onRestore: (toolId: ToolId) => Promise<ConnectionResponse>;
  onRepair: (toolId: ToolId) => void;
  onJoin: () => void;
  onClose: () => void;
}

type Said = { text: string; mood: Mood };

/**
 * 设置：换站点、体检、声音和语言、把应用改回原样、更新。
 *
 * 每一行都是「左边一句人话，右边一个按钮」。会改掉东西的两件事
 * （退出登录、把应用改回原样）点第一下只是问一句，说清后果，再点才做。
 */
export function Settings({
  open,
  who,
  reach,
  host,
  viaKey,
  connections,
  restoring,
  appName,
  checkup,
  onConnect,
  onLogout,
  onRestore,
  onRepair,
  onJoin,
  onClose,
}: Props) {
  const { t, i18n } = useTranslation();
  const update = useDesktopUpdate();
  const [said, setSaid] = useState<Said | null>(null);
  const [asking, setAsking] = useState<"logout" | ToolId | null>(null);
  const [leaving, setLeaving] = useState(false);
  const [muted, setMute] = useState(isMuted);
  const [copied, setCopied] = useState<"yes" | "no" | null>(null);
  useEffect(() => {
    if (open) return;
    setSaid(null);
    setAsking(null);
    setCopied(null);
  }, [open]);

  const language = (SUPPORTED_LANGUAGES as readonly string[]).includes(
    i18n.language,
  )
    ? (i18n.language as Language)
    : "zh";
  const speak = (next: Language) => {
    if (next === language) return;
    // 先记下再切：切的过程里出了岔子，下次打开至少是用户选的那个。
    storeLanguage(next);
    applyHtmlLang(next);
    announceLanguageToNative(next);
    void i18n.changeLanguage(next);
  };

  const logout = async () => {
    setLeaving(true);
    const done = await onLogout().catch(() => false);
    setLeaving(false);
    setAsking(null);
    setSaid(
      done
        ? { text: t("kaifan.settings.loggedOut"), mood: "idle" }
        : { text: t("kaifan.settings.logoutFailed"), mood: "panic" },
    );
  };

  const restore = async (connection: ToolConnection) => {
    const app = appName(connection.toolId);
    setAsking(null);
    try {
      const result = await onRestore(connection.toolId);
      const key = undoKey(result, connection.restoreMode);
      setSaid({
        text: t(`kaifan.settings.${key}`, { app }),
        mood: key === "restoreFailed" ? "panic" : "happy",
      });
    } catch {
      setSaid({
        text: t("kaifan.settings.restoreError", { app }),
        mood: "panic",
      });
    }
  };

  const joined = connections.filter(
    (connection) =>
      connection.restoreMode !== "none" &&
      ["connected", "legacy", "changed", "recovery_pending"].includes(
        connection.state,
      ),
  );

  // —— 体检 ——
  const report = checkup.report;
  const checkSays = (): Said | null => {
    if (checkup.running)
      return { text: t("kaifan.settings.check.running"), mood: "idle" };
    if (!report) return null;
    if (report.verdict === "fine")
      return {
        text: t(
          report.apps === "unknown" || report.apps === "partial"
            ? "kaifan.settings.check.fineBlind"
            : "kaifan.settings.check.fine",
        ),
        mood: "happy",
      };
    return {
      text: t(`kaifan.settings.check.${report.verdict}`, {
        host: host ?? t("kaifan.settings.theStation"),
        apps: (report.verdict === "model" ? report.gone : report.repair)
          .map(appName)
          .join(t("kaifan.listSeparator")),
      }),
      mood: "panic",
    };
  };
  const copy = async () => {
    if (!report) return;
    try {
      await navigator.clipboard.writeText(
        reportText(report, connections, update.projection.currentVersion),
      );
      setCopied("yes");
    } catch {
      setCopied("no");
    }
  };
  const act =
    !report || checkup.running
      ? null
      : report.action === "connect"
        ? { label: t("kaifan.settings.reconnect"), run: onConnect }
        : report.action === "again"
          ? { label: t("kaifan.settings.check.again"), run: checkup.run }
          : report.action === "repair" && report.repair[0]
            ? {
                label: t("kaifan.settings.check.repairGo", {
                  app: appName(report.repair[0]),
                }),
                run: () => onRepair(report.repair[0]!),
              }
            : report.action === "swap" && report.gone[0]
              ? {
                  label: t("kaifan.settings.check.swapGo", {
                    app: appName(report.gone[0]),
                  }),
                  run: () => onRepair(report.gone[0]!),
                }
              : report.action === "join"
                ? { label: t("kaifan.settings.check.join"), run: onJoin }
                : null;
  const mark = (state: "ok" | "bad" | "skip") => (
    <i className={`mk ${state}`} aria-hidden="true">
      {state === "ok" ? "✓" : state === "bad" ? "!" : "–"}
    </i>
  );

  // —— 更新 ——
  const p = update.projection;
  const percent =
    p.phase === "downloading" && p.totalBytes > 0
      ? Math.min(100, Math.round((p.downloadedBytes / p.totalBytes) * 100))
      : null;
  // 自己打的测试版没有更新地址，「再试一次」也查不到，直接让人去官网。
  const unconfigured =
    p.phase === "unavailable" && p.reasonCode === "updater_not_configured";
  const updateNote = unconfigured
    ? t("kaifan.settings.update.notConfigured")
    : p.phase === "available"
      ? t("kaifan.settings.update.available", { version: p.availableVersion })
      : p.phase === "downloading"
        ? percent === null
          ? t("kaifan.settings.update.downloading")
          : t("kaifan.settings.update.downloadingPercent", { percent })
        : t(
            `kaifan.settings.update.${p.phase === "idle" ? "checking" : p.phase}`,
          );
  const stuck = p.phase === "unavailable" || p.phase === "failed";

  // 最近发生的事优先说：刚做完的操作，其次是体检，最后才是开场白。
  const current =
    asking === "logout"
      ? { text: t("kaifan.settings.askLogout"), mood: "idle" as Mood }
      : asking
        ? {
            text: t(askKey(joined.find((c) => c.toolId === asking)), {
              app: appName(asking),
            }),
            mood: "idle" as Mood,
          }
        : (said ??
          checkSays() ?? {
            text: t("kaifan.settings.says"),
            mood: "happy" as Mood,
          });

  return (
    <Drawer
      open={open}
      testId="kaifan-settings"
      label={t("kaifan.nav.settings")}
      who={who}
      mood={current.mood}
      says={current.text}
      onClose={onClose}
    >
      <section>
        <h4>{t("kaifan.settings.station")}</h4>
        <div className="set" data-testid="kaifan-settings-station">
          <span>
            {reach === "out"
              ? t("kaifan.nav.stationOut")
              : (host ?? t("kaifan.settings.builtin"))}
            {reach !== "out" && (
              <small>
                {reach === "off"
                  ? t("kaifan.nav.stationOff")
                  : reach === "wait"
                    ? t("kaifan.nav.stationChecking")
                    : t(
                        viaKey
                          ? "kaifan.settings.viaKey"
                          : "kaifan.settings.viaLogin",
                      )}
              </small>
            )}
          </span>
          <button type="button" className="sb" onClick={onConnect}>
            {t(
              reach === "out"
                ? "kaifan.home.outGo"
                : "kaifan.settings.switchStation",
            )}
          </button>
        </div>
        {reach === "ok" && (
          <div className="set">
            <span>
              {t("kaifan.settings.logout")}
              <small>{t("kaifan.settings.logoutNote")}</small>
            </span>
            {asking === "logout" ? (
              <span className="ask">
                <button
                  type="button"
                  className="sb warn"
                  disabled={leaving}
                  data-testid="kaifan-settings-logout-yes"
                  onClick={() => void logout()}
                >
                  {t("kaifan.settings.logoutYes")}
                </button>
                <button
                  type="button"
                  className="sb"
                  disabled={leaving}
                  onClick={() => setAsking(null)}
                >
                  {t("kaifan.settings.never")}
                </button>
              </span>
            ) : (
              <button
                type="button"
                className="sb"
                data-testid="kaifan-settings-logout"
                onClick={() => {
                  setSaid(null);
                  setAsking("logout");
                }}
              >
                {t("kaifan.settings.logoutAction")}
              </button>
            )}
          </div>
        )}
      </section>

      {/* 体检查出登录失效时首页会变成「没连站点」，结果和「重新连接」仍然要留着。 */}
      {(reach !== "out" || report || checkup.running) && (
        <section>
          <h4>{t("kaifan.settings.check.title")}</h4>
          <div className="set">
            <span>
              {t("kaifan.settings.check.name")}
              <small>{t("kaifan.settings.check.note")}</small>
            </span>
            <button
              type="button"
              className="sb"
              disabled={checkup.running}
              data-testid="kaifan-settings-check"
              onClick={() => {
                setSaid(null);
                setAsking(null);
                checkup.run();
              }}
            >
              {t(
                checkup.running
                  ? "kaifan.settings.check.busy"
                  : report
                    ? "kaifan.settings.check.rerun"
                    : "kaifan.settings.check.start",
              )}
            </button>
          </div>
          {report && !checkup.running && (
            <>
              <ul className="chk" data-testid="kaifan-settings-report">
                <li>
                  {mark(report.station === "ok" ? "ok" : "bad")}
                  {t(`kaifan.settings.check.station.${report.station}`)}
                </li>
                <li>
                  {mark(
                    report.login === "ok"
                      ? "ok"
                      : report.login === "skipped"
                        ? "skip"
                        : "bad",
                  )}
                  {t(`kaifan.settings.check.login.${report.login}`)}
                </li>
                <li>
                  {mark(
                    report.apps === "ok"
                      ? "ok"
                      : report.apps === "repair" || report.apps === "model"
                        ? "bad"
                        : "skip",
                  )}
                  {t(`kaifan.settings.check.apps.${report.apps}`, {
                    count:
                      report.apps === "repair"
                        ? report.repair.length
                        : report.apps === "model"
                          ? report.gone.length
                          : report.live,
                  })}
                </li>
              </ul>
              {act && (
                <button
                  type="button"
                  className="go"
                  data-testid="kaifan-settings-check-act"
                  onClick={act.run}
                >
                  {act.label}
                </button>
              )}
              <div className="set">
                <span>
                  {t("kaifan.settings.check.copyNote")}
                  <small role="status">
                    {copied && t(`kaifan.settings.check.copied.${copied}`)}
                  </small>
                </span>
                <button
                  type="button"
                  className="sb"
                  data-testid="kaifan-settings-check-copy"
                  onClick={() => void copy()}
                >
                  {t("kaifan.settings.check.copy")}
                </button>
              </div>
            </>
          )}
        </section>
      )}

      <section>
        <h4>{t("kaifan.settings.look")}</h4>
        <div className="set">
          <span>{t("kaifan.settings.sound")}</span>
          <button
            type="button"
            className="tg"
            role="switch"
            aria-checked={!muted}
            aria-label={t("kaifan.settings.sound")}
            data-testid="kaifan-settings-sound"
            onClick={() => {
              setMuted(!muted);
              setMute(!muted);
              if (muted) sound.pop(620);
            }}
          />
        </div>
        <div className="set">
          <span>{t("kaifan.settings.language")}</span>
          <span className="ask">
            {SUPPORTED_LANGUAGES.map((id) => (
              <button
                type="button"
                className="sb"
                key={id}
                lang={id}
                aria-pressed={language === id}
                onClick={() => speak(id)}
              >
                {LANGUAGE_NAME[id]}
              </button>
            ))}
          </span>
        </div>
      </section>

      {joined.length > 0 && (
        <section>
          <h4>{t("kaifan.settings.apps")}</h4>
          <p className="nt">{t("kaifan.settings.appsNote")}</p>
          {joined.map((connection) => {
            const original = connection.restoreMode === "original";
            const busy = restoring === connection.toolId;
            return (
              <div
                className="set"
                key={connection.toolId}
                data-testid={`kaifan-settings-app-${connection.toolId}`}
              >
                <span>{appName(connection.toolId)}</span>
                {asking === connection.toolId ? (
                  <span className="ask">
                    <button
                      type="button"
                      className="sb warn"
                      onClick={() => void restore(connection)}
                    >
                      {t(
                        original
                          ? "kaifan.settings.restoreYes"
                          : "kaifan.settings.removeYes",
                      )}
                    </button>
                    <button
                      type="button"
                      className="sb"
                      onClick={() => setAsking(null)}
                    >
                      {t("kaifan.settings.never")}
                    </button>
                  </span>
                ) : (
                  <button
                    type="button"
                    className="sb"
                    disabled={!!restoring}
                    onClick={() => {
                      setSaid(null);
                      setAsking(connection.toolId);
                    }}
                  >
                    {t(
                      busy
                        ? "kaifan.settings.restoring"
                        : original
                          ? "kaifan.settings.restoreAction"
                          : "kaifan.settings.removeAction",
                    )}
                  </button>
                )}
              </div>
            );
          })}
        </section>
      )}

      <section>
        <h4>{t("kaifan.settings.privacy")}</h4>
        <ul className="chk" data-testid="kaifan-settings-privacy">
          {(["keys", "apps", "reports"] as const).map((item) => (
            <li key={item}>{t(`kaifan.settings.privacyNote.${item}`)}</li>
          ))}
        </ul>
      </section>

      <section>
        <h4>{t("kaifan.settings.about")}</h4>
        <div className="set" data-testid="kaifan-settings-update">
          <span>
            {p.currentVersion
              ? t("kaifan.settings.update.version", {
                  version: p.currentVersion,
                })
              : t("kaifan.settings.update.versionUnknown")}
            <small role="status">{updateNote}</small>
          </span>
          {p.phase === "available" ? (
            <button type="button" className="sb hot" onClick={update.install}>
              {t("kaifan.settings.update.install")}
            </button>
          ) : (
            <button
              type="button"
              className="sb"
              disabled={update.working || p.phase === "idle" || unconfigured}
              onClick={update.check}
            >
              {t(
                stuck && !unconfigured
                  ? "kaifan.settings.update.retry"
                  : "kaifan.settings.update.check",
              )}
            </button>
          )}
        </div>
        {percent !== null && (
          <progress className="pg" value={percent} max={100} />
        )}
        {stuck && (
          <p className="nt">
            {unconfigured ? "" : `${t("kaifan.settings.update.manual")} `}
            <a
              className="lnk"
              href={DESKTOP_DOWNLOAD_PAGE_URL}
              target="_blank"
              rel="noreferrer"
            >
              {t("kaifan.settings.update.manualLink")}
            </a>
          </p>
        )}
      </section>
      <section>
        <div className="set" data-testid="kaifan-settings-quit">
          <span>
            {t("kaifan.settings.quit")}
            <small>{t("kaifan.settings.quitNote")}</small>
          </span>
          <button type="button" className="sb" onClick={askToQuit}>
            {t("kaifan.settings.quitAction")}
          </button>
        </div>
      </section>
    </Drawer>
  );
}
