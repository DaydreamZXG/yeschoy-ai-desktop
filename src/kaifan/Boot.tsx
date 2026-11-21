import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ROSTER, ROSTER_IDS } from "./roster";
import { prefersCalm, sound } from "./sound";

interface Props {
  /** 站点的域名；还没连站点时没有。 */
  station: string | null;
  /** 这个站上到齐了几位；还不知道时不报数。 */
  castCount: number | null;
  onDone: () => void;
}

interface Step {
  index: number;
  title: string;
  caption: string;
  big?: boolean;
}

/**
 * 开机：四步，约六秒，随时可以跳过。
 *
 * 每一步说的都是真的在发生的事；还不知道的（站点、到了几位）就不说，
 * 不拿演示数字顶上。系统里关了动效的人不播，直接进首页。
 */
export function Boot({ station, castCount, onDone }: Props) {
  const { t } = useTranslation();
  const [step, setStep] = useState<Step | null>(null);
  const [caption, setCaption] = useState("");
  const [dots, setDots] = useState(0);
  const [open, setOpen] = useState(false);
  const [clock] = useState(() =>
    new Date().toLocaleTimeString("en-GB", { hour12: false }),
  );
  const done = useRef(onDone);
  done.current = onDone;
  // 开机开始时知道多少就说多少，播到一半不改口。
  const facts = useRef({ station, castCount });

  useEffect(() => {
    if (prefersCalm()) {
      done.current();
      return;
    }
    const { station: host, castCount: count } = facts.current;
    const timers: ReturnType<typeof setTimeout>[] = [];
    const at = (ms: number, run: () => void) =>
      timers.push(setTimeout(run, ms));
    sound.drone(5.2);
    at(700, () =>
      setStep({
        index: 1,
        title: t(host ? "kaifan.boot.connecting" : "kaifan.boot.starting"),
        caption: host ? `CONNECTING · ${host.toUpperCase()}` : "STARTING UP",
      }),
    );
    at(1700, () => {
      sound.ok();
      setStep({
        index: 2,
        title: t("kaifan.boot.checking"),
        caption: "CHECKING APPS",
      });
    });
    at(2700, () => {
      sound.ok();
      setStep({
        index: 3,
        title: count
          ? t("kaifan.boot.cast", { count })
          : t("kaifan.boot.castUnknown"),
        caption: count ? `${count} ONLINE` : "ROLL CALL",
      });
    });
    ROSTER_IDS.forEach((_, i) =>
      at(2850 + i * 95, () => {
        setDots(i + 1);
        sound.tone(520 * Math.pow(2, i / 9), 0.09, "sine", 0.13);
      }),
    );
    at(3200, sound.sweep);
    at(4000, () => {
      setStep({
        index: 4,
        title: t("kaifan.boot.welcome"),
        caption: "WELCOME BACK",
        big: true,
      });
      sound.impact();
      sound.chord([220, 329.6, 440, 493.9], 3.2, 0.02);
    });
    at(5000, () => {
      setOpen(true);
      sound.whoosh();
    });
    at(5700, () => done.current());
    const skip = (event: KeyboardEvent) => {
      if (event.key === "Escape") done.current();
    };
    window.addEventListener("keydown", skip);
    return () => {
      timers.forEach(clearTimeout);
      window.removeEventListener("keydown", skip);
    };
  }, [t]);

  // 英文小字一个个打出来。
  useEffect(() => {
    if (!step) return;
    let shown = 0;
    setCaption("");
    const timer = setInterval(() => {
      shown += 1;
      setCaption(step.caption.slice(0, shown));
      sound.tick();
      if (shown >= step.caption.length) clearInterval(timer);
    }, 24);
    return () => clearInterval(timer);
  }, [step]);

  const index = step?.index ?? 0;
  return (
    <div
      className={open ? "boot open" : "boot"}
      data-testid="kaifan-boot"
      aria-hidden="true"
    >
      <div className="veil" />
      <div className="bx">
        <svg className="ring" viewBox="0 0 100 100">
          <circle cx="50" cy="50" r="46" />
          <circle className="o2" cx="50" cy="50" r="49" />
        </svg>
        <i className="cn a" />
        <i className="cn b" />
        <i className="cn c" />
        <i className="cn d" />
        <p className="bk tl">
          <b>KAIFAN</b> · DESKTOP
          <br />
          {clock}
        </p>
        <p className="bk br">{`[ 0${index} / 04 ]`}</p>
        <div className="bc">
          <h1 className={step?.big ? "big" : undefined} key={index}>
            {[...(step?.title ?? "")].map((char, i) => (
              <span key={i} style={{ ["--i" as string]: i }}>
                {char === " " ? "\u00a0" : char}
              </span>
            ))}
          </h1>
          <p>{caption}</p>
          <div className="dots">
            {ROSTER_IDS.map((id, i) => (
              <i
                key={id}
                className={i < dots ? "on" : undefined}
                style={{ ["--c" as string]: ROSTER[id].color }}
              />
            ))}
          </div>
        </div>
        <div className="bp">
          {[1, 2, 3, 4].map((n) => (
            <i key={n} className={n <= index ? "on" : undefined} />
          ))}
        </div>
        <button
          type="button"
          className="skip"
          tabIndex={-1}
          onClick={() => done.current()}
        >
          {t("kaifan.boot.skip")}
        </button>
      </div>
    </div>
  );
}
