import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { openStationConnectPage } from "../account/openStationConnectPage";
import { connectWithKey } from "../account/session";
import {
  STATION_KEY_LINE_PREFIX,
  type AccountLineId,
} from "../configuration/preview";
import { BUILTIN_STATION_ORIGINS } from "../station/stationProbe";
import { appDisplayName, findApp } from "../workbench/appCatalog";
import {
  useProviderPresets,
  vendorFamilies,
  vendorName,
  type VendorFamily,
} from "./providers";
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

type Step =
  | "url"
  | "checking"
  | "trust"
  | "browser"
  | "key"
  | "vendor"
  | "working";

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
 *
 * 第一步分两组：中转站填网址；云厂商点名字，地址是内置的，只要粘贴密钥。
 * 不常用的厂商收在「还有几家」里，不一上来全铺开。
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
  const families = vendorFamilies(useProviderPresets());
  const [vendor, setVendor] = useState<VendorFamily | null>(null);
  const [region, setRegion] = useState(0);
  const [moreCloud, setMoreCloud] = useState(false);
  const preset = step === "vendor" ? (vendor?.regions[region] ?? null) : null;

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
    setVendor(null);
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
    if (step === "key" || step === "vendor") keyInput.current?.focus();
  }, [open, step]);

  const pickVendor = (family: VendorFamily, at = 0) => {
    const picked = family.regions[at];
    if (!picked) return;
    run.current += 1;
    setBusy(false);
    setVendor(family);
    setRegion(at);
    setOrigin(picked.origin);
    setStep("vendor");
    sound.pop();
    say(t("kaifan.connect.vendorAsk", { name: vendorName(picked, t) }));
  };

  const openKeyPage = async () => {
    if (!preset) return;
    try {
      await invoke("provider_open_key_page_v1", { id: preset.id });
    } catch {
      say(t("kaifan.connect.keyPageFailed"), "panic");
    }
  };

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
      // 线路等授权真做完了再换（`useStationLink` 收到结果时换）：现在就换的话，
      // 本机存的还是原来那个站，界面会被拉回去，授权成功了反而报失败。
      setStep("browser");
      say(t("kaifan.connect.inBrowser"), "happy");
    },
    [say, t, toKey],
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
    const fail = (code: string) => {
      setBusy(false);
      say(t(`kaifan.connect.key.${code}`), "panic");
    };
    // 云厂商不先探一遍：那一步问的是中转站才有的接口。连接本身就会核对密钥。
    if (!preset) {
      const outcome = await lookAtStation(origin, token);
      if (current !== run.current) return;
      if (!outcome.ok) return fail("error");
      if (outcome.probe.tokenStatus !== "valid")
        return fail(
          ["invalid", "no_quota"].includes(outcome.probe.tokenStatus)
            ? outcome.probe.tokenStatus
            : "error",
        );
    }
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
    const left = current !== run.current;
    if (failure) return left ? undefined : fail(failure);
    // 走到这里，本机已经存好新密钥、换到这个站了。中途关了面板或者点了上一步
    // 也一样要跟过去，不然界面还停在原来那个站上，和本机实际连的对不上。
    if (!left) {
      // 存好就从输入框里清掉，不在界面上露着。
      setKey("");
      setBusy(false);
    }
    onLineChange(`${STATION_KEY_LINE_PREFIX}${origin}`);
    onConnected({
      host: preset ? vendorName(preset, t) : hostOf(origin),
      viaKey: true,
    });
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
          <>
            <section className="grp" aria-labelledby="kf-relay">
              <h3 id="kf-relay">
                {t("kaifan.connect.groupRelay")}
                <small>{t("kaifan.connect.groupRelayNote")}</small>
              </h3>
              <form
                className="row"
                onSubmit={(event) => {
                  event.preventDefault();
                  void check();
                }}
              >
                <label htmlFor="kf-url" className="vh">
                  {t("kaifan.connect.urlLabel")}
                </label>
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
                <button type="submit" className="go" disabled={busy}>
                  {t("kaifan.connect.next")}
                </button>
              </form>
              <p className="hint">{t("kaifan.connect.urlHint")}</p>
            </section>
            {families.length > 0 && (
              <section className="grp" aria-labelledby="kf-cloud">
                <h3 id="kf-cloud">
                  {t("kaifan.connect.groupCloud")}
                  <small>{t("kaifan.connect.groupCloudNote")}</small>
                </h3>
                <div className="vds" data-testid="kaifan-connect-vendors">
                  {families
                    .filter((family) => moreCloud || family.common)
                    .map((family) => (
                      <button
                        key={family.family}
                        type="button"
                        className="vd"
                        onClick={() => pickVendor(family)}
                      >
                        <b>{vendorName(family.regions[0], t)}</b>
                        <small>
                          {t(
                            family.billing === "plan"
                              ? "kaifan.connect.tagPlan"
                              : "kaifan.connect.tagPayg",
                          )}
                        </small>
                      </button>
                    ))}
                </div>
                {families.some((family) => !family.common) && (
                  <button
                    type="button"
                    className="fold"
                    aria-expanded={moreCloud}
                    onClick={() => setMoreCloud((shown) => !shown)}
                  >
                    {moreCloud
                      ? t("kaifan.connect.less")
                      : t("kaifan.connect.more", {
                          count: families.filter((family) => !family.common)
                            .length,
                        })}
                  </button>
                )}
              </section>
            )}
          </>
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

        {step === "vendor" && vendor && preset && (
          <form
            style={{ display: "contents" }}
            onSubmit={(event) => {
              event.preventDefault();
              void connectKey();
            }}
          >
            {vendor.regions.length > 1 && (
              <p
                className="seg"
                role="group"
                aria-label={t("kaifan.connect.region")}
              >
                {vendor.regions.map((item, at) => (
                  <button
                    key={item.id}
                    type="button"
                    aria-pressed={at === region}
                    disabled={busy}
                    onClick={() => pickVendor(vendor, at)}
                  >
                    {t(`kaifan.connect.region_${item.region ?? "cn"}`)}
                  </button>
                ))}
              </p>
            )}
            <div>
              <label htmlFor="kf-key">{t("kaifan.connect.keyLabel")}</label>
              <input
                id="kf-key"
                ref={keyInput}
                className="inp"
                type="password"
                value={key}
                onChange={(event) => setKey(event.target.value)}
                placeholder={t("kaifan.connect.vendorKeyPlaceholder")}
                autoComplete="off"
                spellCheck={false}
                disabled={busy}
              />
            </div>
            {preset.billing === "plan" ? (
              <p
                className="quote warn"
                role="note"
                data-testid="kaifan-connect-risk"
              >
                <b>{t("kaifan.connect.planRiskTitle")}</b>
                <br />
                {t("kaifan.connect.planRisk", { name: vendorName(preset, t) })}
              </p>
            ) : (
              <p className="hint">{t("kaifan.connect.paygNote")}</p>
            )}
            {(preset.onlyTools.length > 0 || preset.unverifiable) && (
              <p className="hint" data-testid="kaifan-connect-limits">
                {preset.onlyTools.length > 0 &&
                  t("kaifan.connect.onlyTools", {
                    apps: preset.onlyTools
                      .map((tool) => {
                        const app = findApp(tool);
                        return app ? appDisplayName(app.id, app.name, t) : tool;
                      })
                      .join(t("kaifan.connect.listJoin")),
                  })}
                {preset.unverifiable && t("kaifan.connect.unverifiable")}
              </p>
            )}
            <div className="ft">
              {back}
              <i />
              <button
                type="button"
                className="go alt"
                onClick={() => void openKeyPage()}
              >
                {t("kaifan.connect.getKey")}
              </button>
              <button type="submit" className="go" disabled={busy}>
                {t(busy ? "kaifan.connect.connecting" : "kaifan.connect.go")}
              </button>
            </div>
          </form>
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
