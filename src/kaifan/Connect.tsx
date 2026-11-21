import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { openStationConnectPage } from "../account/openStationConnectPage";
import { connectWithKey } from "../account/session";
import {
  STATION_KEY_LINE_PREFIX,
  type AccountLineId,
} from "../configuration/preview";
import { BUILTIN_STATION_ORIGINS } from "../station/stationProbe";
import { ROSTER, chibiOf, type Mood } from "./roster";
import { sound } from "./sound";
import {
  hostOf,
  hostOfLine,
  lookAtStation,
  normalizeStationInput,
} from "./station";
import type { LinkEvent } from "./useStationLink";
import { useTyped } from "./useTyped";

type Step = "url" | "checking" | "trust" | "browser" | "key" | "working";

interface Props {
  open: boolean;
  /** 已经连着一个站点时可以关掉；一个站点都没连时没有别处可去，不给关。 */
  dismissible: boolean;
  /** 浏览器授权那边传回来的最新进展。 */
  linkEvent: (LinkEvent & { at: number }) | null;
  onLineChange: (lineId: AccountLineId) => void;
  /** 连上了。viaKey：是粘贴密钥连的，余额这些不一定看得到。 */
  onConnected: (result: { host: string | null; viaKey: boolean }) => void;
  /** 面板没开着时有话要说（比如网页那边发起的连接没成），交给首页的角色说。 */
  onNotice: (text: string, mood?: Mood) => void;
  onClose: () => void;
}

/**
 * 连接站点：阿青带着走。填网址 → 她去看这个站 → 能一键登录就去浏览器点「允许」，
 * 不能就粘贴密钥。用户不用知道站点是哪一种，也不用自己选。
 */
export function Connect({
  open,
  dismissible,
  linkEvent,
  onLineChange,
  onConnected,
  onNotice,
  onClose,
}: Props) {
  const { t } = useTranslation();
  const guide = ROSTER.gpt;
  const [step, setStep] = useState<Step>("url");
  const [origin, setOrigin] = useState("");
  const [url, setUrl] = useState("");
  const [key, setKey] = useState("");
  const [line, setLine] = useState("");
  const [mood, setMood] = useState<Mood>("idle");
  const [busy, setBusy] = useState(false);
  const [ticks, setTicks] = useState(0);
  const run = useRef(0);
  const urlInput = useRef<HTMLInputElement>(null);
  const keyInput = useRef<HTMLInputElement>(null);
  const typed = useTyped(line);

  const say = useCallback((text: string, face: Mood = "idle") => {
    setLine(text);
    setMood(face);
    if (face === "panic") sound.sad();
  }, []);

  const toUrl = useCallback(() => {
    run.current += 1;
    setBusy(false);
    setStep("url");
    setKey("");
    say(t("kaifan.connect.askUrl"));
  }, [say, t]);

  useEffect(() => {
    if (!open) return;
    sound.whoosh();
    toUrl();
  }, [open, toUrl]);

  useEffect(() => {
    if (!open) return;
    if (step === "url") urlInput.current?.focus();
    if (step === "key") keyInput.current?.focus();
  }, [open, step]);

  const toKey = useCallback(
    (text: string, face: Mood = "idle") => {
      setStep("key");
      say(text, face);
    },
    [say],
  );

  const openBrowser = useCallback(
    async (target: string) => {
      const current = ++run.current;
      setBusy(true);
      const result = await openStationConnectPage(target);
      if (current !== run.current) return;
      setBusy(false);
      if (!result.ok) {
        // 浏览器打不开时不让人卡在这里：密钥这条路照样能连。
        toKey(t("kaifan.connect.browserFailed"), "panic");
        return;
      }
      onLineChange(result.lineId);
      setStep("browser");
      say(t("kaifan.connect.inBrowser"), "happy");
    },
    [onLineChange, say, t, toKey],
  );

  const check = async () => {
    const target = normalizeStationInput(url);
    if (!url.trim()) return say(t("kaifan.connect.urlEmpty"), "panic");
    if (!target) return say(t("kaifan.connect.urlBad"), "panic");
    setOrigin(target);
    if (BUILTIN_STATION_ORIGINS.includes(target)) {
      // 内置的参考站点不用探测，但去浏览器之前一样先问一句。
      setStep("trust");
      return say(t("kaifan.connect.trust", { host: hostOf(target) }));
    }
    const current = ++run.current;
    setStep("checking");
    setTicks(0);
    say(t("kaifan.connect.looking", { host: hostOf(target) }));
    const outcome = await lookAtStation(target);
    if (current !== run.current) return;
    if (!outcome.ok) {
      setStep("url");
      say(
        t(`kaifan.connect.failed.${outcome.reason}`, {
          defaultValue: t("kaifan.connect.failed.unreachable"),
        }),
        "panic",
      );
      return;
    }
    setTicks(2);
    sound.ok();
    if (outcome.probe.desktopConnect) {
      setStep("trust");
      say(t("kaifan.connect.trust", { host: hostOf(target) }));
    } else toKey(t("kaifan.connect.askKey"));
  };

  const connectKey = async () => {
    const token = key.trim();
    if (!token) return say(t("kaifan.connect.keyEmpty"), "panic");
    const current = ++run.current;
    setBusy(true);
    say(t("kaifan.connect.trying"));
    const outcome = await lookAtStation(origin, token);
    if (current !== run.current) return;
    const fail = (code: string) => {
      setBusy(false);
      say(t(`kaifan.connect.key.${code}`), "panic");
    };
    if (!outcome.ok) return fail("error");
    if (outcome.probe.tokenStatus !== "valid")
      return fail(
        ["invalid", "no_quota"].includes(outcome.probe.tokenStatus)
          ? outcome.probe.tokenStatus
          : "error",
      );
    let failure: string | null = null;
    try {
      const projection = await connectWithKey(origin, token);
      if (projection.status !== "signed_in")
        failure =
          projection.status === "secure_storage_unavailable"
            ? "storage"
            : projection.reasonCode === "key_rejected"
              ? "rejected"
              : "failed";
    } catch {
      failure = "failed";
    }
    if (current !== run.current) return;
    if (failure) return fail(failure);
    // 存好就从输入框里清掉，不在界面上露着。
    setKey("");
    setBusy(false);
    onLineChange(`${STATION_KEY_LINE_PREFIX}${origin}`);
    onConnected({ host: hostOf(origin), viaKey: true });
  };

  // 浏览器那边有结果了。
  const seen = useRef(0);
  useEffect(() => {
    if (!linkEvent || linkEvent.at === seen.current) return;
    seen.current = linkEvent.at;
    if (!open || (step !== "browser" && step !== "working")) {
      // 不是从这里发起的（比如在站点网页上点了「打开客户端」）：结果一样要让人看到。
      if (linkEvent.phase === "signed_in")
        return onConnected({
          host: linkEvent.lineId ? hostOfLine(linkEvent.lineId) : null,
          viaKey: false,
        });
      const failed = linkEvent.phase === "failed";
      const text = failed
        ? t(`stationConnect.reason.${linkEvent.reasonCode}`, {
            defaultValue: t("kaifan.connect.strayFailed"),
          })
        : t("kaifan.connect.trying");
      return open
        ? say(text, failed ? "panic" : "idle")
        : onNotice(text, failed ? "panic" : undefined);
    }
    if (linkEvent.phase === "connecting") {
      setStep("working");
      say(t("kaifan.connect.trying"));
    } else if (linkEvent.phase === "signed_in") {
      onConnected({ host: hostOf(origin), viaKey: false });
    } else if (linkEvent.reasonCode === "connect_loopback_unavailable") {
      toKey(t("kaifan.connect.askKey"));
    } else {
      setStep("browser");
      say(
        t(`stationConnect.reason.${linkEvent.reasonCode}`, {
          defaultValue: t("kaifan.connect.browserRetry"),
        }),
        "panic",
      );
    }
  }, [linkEvent, onConnected, onNotice, open, origin, say, step, t, toKey]);

  const close = () => {
    run.current += 1;
    onClose();
  };
  const back = (
    <button
      type="button"
      className="go alt"
      // 确认站点这一步，回车默认是「不连」。
      autoFocus={step === "trust"}
      onClick={toUrl}
    >
      {t("kaifan.connect.back")}
    </button>
  );

  return (
    <div
      className={open ? "sheet on" : "sheet"}
      data-testid="kaifan-connect"
      data-step={step}
      aria-hidden={!open}
      onKeyDown={(event) => {
        if (event.key === "Escape" && dismissible) close();
      }}
    >
      <div
        className="panel glass cnp"
        role="dialog"
        aria-modal="true"
        aria-label={t("kaifan.connect.title")}
      >
        {dismissible && (
          <button
            type="button"
            className="x"
            aria-label={t("kaifan.close")}
            onClick={close}
          >
            ×
          </button>
        )}
        <header className="dh">
          <img src={chibiOf("gpt", mood)} alt="" />
          <div>
            <small className="who" style={{ ["--hc" as string]: guide.color }}>
              {t("roster.gpt.name")}
              <small>{guide.brand}</small>
            </small>
            <p aria-live="polite" data-testid="kaifan-connect-says">
              {typed}
            </p>
          </div>
        </header>

        {step === "url" && (
          <form
            style={{ display: "contents" }}
            onSubmit={(event) => {
              event.preventDefault();
              void check();
            }}
          >
            <div>
              <label htmlFor="kf-url">{t("kaifan.connect.urlLabel")}</label>
              <input
                id="kf-url"
                ref={urlInput}
                className="inp"
                value={url}
                onChange={(event) => setUrl(event.target.value)}
                placeholder={t("kaifan.connect.urlPlaceholder")}
                autoComplete="off"
                spellCheck={false}
                disabled={busy}
              />
            </div>
            <p className="hint">{t("kaifan.connect.urlHint")}</p>
            <div className="ft">
              <i />
              <button type="submit" className="go" disabled={busy}>
                {t("kaifan.connect.next")}
              </button>
            </div>
          </form>
        )}

        {step === "checking" && (
          <>
            <ol className="cl">
              <li className={ticks > 0 ? "ok" : undefined}>
                {t("kaifan.connect.stepFind")}
              </li>
              <li className={ticks > 1 ? "ok" : undefined}>
                {t("kaifan.connect.stepLogin")}
              </li>
            </ol>
            <div className="ft">
              {back}
              <i />
            </div>
          </>
        )}

        {step === "trust" && (
          <>
            <p className="quote">
              <b>{hostOf(origin)}</b>
              <br />
              {t("kaifan.connect.trustNote")}
            </p>
            <div className="ft">
              {back}
              <i />
              <button
                type="button"
                className="go"
                disabled={busy}
                onClick={() => void openBrowser(origin)}
              >
                {t("kaifan.connect.trustGo")}
              </button>
            </div>
          </>
        )}

        {(step === "browser" || step === "working") && (
          <>
            <ol className="cl">
              <li className="ok">{t("kaifan.connect.stepOpened")}</li>
              <li className={step === "working" ? "ok" : undefined}>
                {t("kaifan.connect.stepAllow")}
              </li>
              <li>{t("kaifan.connect.stepBack")}</li>
            </ol>
            <p className="hint">{t("kaifan.connect.browserHint")}</p>
            <div className="ft">
              {back}
              <i />
              <button
                type="button"
                className="go alt"
                onClick={() => toKey(t("kaifan.connect.askKeyInstead"))}
              >
                {t("kaifan.connect.useKey")}
              </button>
              <button
                type="button"
                className="go"
                disabled={busy}
                onClick={() => void openBrowser(origin)}
              >
                {t("kaifan.connect.reopen")}
              </button>
            </div>
          </>
        )}

        {step === "key" && (
          <form
            style={{ display: "contents" }}
            onSubmit={(event) => {
              event.preventDefault();
              void connectKey();
            }}
          >
            <div>
              <label htmlFor="kf-key">{t("kaifan.connect.keyLabel")}</label>
              <input
                id="kf-key"
                ref={keyInput}
                className="inp"
                type="password"
                value={key}
                onChange={(event) => setKey(event.target.value)}
                placeholder={t("kaifan.connect.keyPlaceholder")}
                autoComplete="off"
                spellCheck={false}
                disabled={busy}
              />
            </div>
            <p className="hint">{t("kaifan.connect.keyHint")}</p>
            <div className="ft">
              {back}
              <i />
              <button type="submit" className="go" disabled={busy}>
                {t(busy ? "kaifan.connect.connecting" : "kaifan.connect.go")}
              </button>
            </div>
          </form>
        )}
      </div>
    </div>
  );
}
