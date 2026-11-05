import { useCallback, useEffect, useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { useTranslation } from "react-i18next";
import type { SkinCharacter } from "./skin";
import { markBootPlayed, readBootPreference } from "./bootPrefs";
import "./bootRitual.css";

export type BootStage = 1 | 2 | 3 | 4;

interface BootRitualProps {
  character: SkinCharacter;
  onComplete: () => void;
  /** Test hook: start at a given stage (1–4). */
  initialStage?: BootStage;
  /** Test hook: disable auto timers. */
  disableAutoAdvance?: boolean;
}

/** Auto dwell per act. Stage 2 waits on BREACH, then auto-advances. */
const STAGE_MS: Record<BootStage, number> = {
  1: 1700,
  2: 5000,
  3: 1600,
  4: 1500,
};

const LEAVE_MS = 420;
const BREACH_FLASH_MS = 320;

const ACT_EASE = [0.22, 1, 0.36, 1] as const;

const actMotion = (reduce: boolean) =>
  reduce
    ? {
        initial: { opacity: 1, y: 0, filter: "none" },
        animate: { opacity: 1, y: 0, filter: "none" },
        exit: { opacity: 0 },
        transition: { duration: 0.12 },
      }
    : {
        initial: { opacity: 0, y: 14, filter: "blur(6px)" },
        animate: { opacity: 1, y: 0, filter: "blur(0px)" },
        exit: { opacity: 0, y: -10, filter: "blur(4px)" },
        transition: { duration: 0.42, ease: ACT_EASE },
      };

export function BootRitual({
  character,
  onComplete,
  initialStage = 1,
  disableAutoAdvance = false,
}: BootRitualProps) {
  const { t } = useTranslation();
  const reduceMotion = useReducedMotion() === true;
  const [stage, setStage] = useState<BootStage>(initialStage);
  const [leaving, setLeaving] = useState(false);
  const [flashing, setFlashing] = useState(false);
  const finished = useRef(false);
  const flashTimer = useRef<number | null>(null);

  const finish = useCallback(() => {
    if (finished.current) return;
    finished.current = true;
    if (flashTimer.current != null) {
      window.clearTimeout(flashTimer.current);
      flashTimer.current = null;
    }
    setFlashing(false);
    markBootPlayed(readBootPreference());
    setLeaving(true);
    window.setTimeout(() => onComplete(), LEAVE_MS);
  }, [onComplete]);

  const advanceFrom = useCallback(
    (from: BootStage) => {
      if (from === 4) {
        finish();
        return;
      }
      setStage((from + 1) as BootStage);
    },
    [finish],
  );

  const breach = useCallback(() => {
    if (stage !== 2 || finished.current || flashing) return;
    setFlashing(true);
    if (flashTimer.current != null) window.clearTimeout(flashTimer.current);
    flashTimer.current = window.setTimeout(() => {
      setFlashing(false);
      flashTimer.current = null;
      advanceFrom(2);
    }, reduceMotion ? 80 : BREACH_FLASH_MS);
  }, [stage, flashing, advanceFrom, reduceMotion]);

  useEffect(() => {
    if (disableAutoAdvance) return;
    if (stage === 2) {
      if (flashing) return;
      const ms = reduceMotion ? 1000 : STAGE_MS[2];
      const id = window.setTimeout(() => breach(), ms);
      return () => window.clearTimeout(id);
    }
    const ms = STAGE_MS[stage];
    const id = window.setTimeout(() => advanceFrom(stage), ms);
    return () => window.clearTimeout(id);
  }, [stage, advanceFrom, disableAutoAdvance, flashing, breach, reduceMotion]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        finish();
        return;
      }
      if (stage === 2 && (event.key === "Enter" || event.key === " ")) {
        event.preventDefault();
        breach();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [stage, finish, breach]);

  useEffect(() => {
    return () => {
      if (flashTimer.current != null) window.clearTimeout(flashTimer.current);
    };
  }, []);

  const titleKey =
    stage === 1
      ? "bootRitual.stage1Title"
      : stage === 2
        ? "bootRitual.stage2Title"
        : stage === 3
          ? "bootRitual.stage3Title"
          : "bootRitual.stage4Title";
  const enKey =
    stage === 1
      ? "bootRitual.stage1En"
      : stage === 2
        ? "bootRitual.stage2En"
        : stage === 3
          ? "bootRitual.stage3En"
          : "bootRitual.stage4En";

  const motionProps = actMotion(reduceMotion);

  return (
    <div
      className="boot-ritual"
      data-testid="boot-ritual"
      data-stage={stage}
      data-character={character}
      data-leaving={leaving ? "true" : "false"}
      data-flashing={flashing ? "true" : "false"}
      role="dialog"
      aria-modal="true"
      aria-labelledby="boot-ritual-title"
    >
      <div className="boot-ritual-wallpaper" aria-hidden="true" />
      <div className="boot-ritual-glass" aria-hidden="true" />
      <div className="boot-ritual-bloom" aria-hidden="true" />
      <div className="boot-ritual-chroma" aria-hidden="true" />

      <button
        type="button"
        className="boot-ritual-skip"
        data-testid="boot-ritual-skip"
        onClick={finish}
      >
        {t("bootRitual.skip")}
      </button>

      <div className="boot-ritual-hud">
        <div className="boot-ritual-crosshair" aria-hidden="true">
          <i className="boot-ritual-reticle" />
          <span />
          <span />
        </div>

        <div className="boot-ritual-progress" aria-hidden="true">
          {([1, 2, 3, 4] as BootStage[]).map((step) => (
            <span
              key={step}
              className="boot-ritual-progress-pip"
              data-state={
                step < stage ? "done" : step === stage ? "active" : "todo"
              }
            />
          ))}
        </div>

        <p className="boot-ritual-index" aria-hidden="true">
          {String(stage).padStart(2, "0")} / 04
        </p>

        <AnimatePresence mode="wait" initial={!reduceMotion}>
          <motion.div
            key={stage}
            className="boot-ritual-act"
            data-testid="boot-ritual-act"
            initial={motionProps.initial}
            animate={motionProps.animate}
            exit={motionProps.exit}
            transition={motionProps.transition}
          >
            <h1 id="boot-ritual-title" className="boot-ritual-title">
              {t(titleKey)}
            </h1>
            <p className="boot-ritual-en" lang="en">
              {t(enKey)}
            </p>

            {stage === 2 && (
              <button
                type="button"
                className="boot-ritual-breach"
                data-testid="boot-ritual-breach"
                onClick={breach}
              >
                {t("bootRitual.breach")}
              </button>
            )}

            {stage === 3 && (
              <div className="boot-ritual-prism" aria-hidden="true" />
            )}
          </motion.div>
        </AnimatePresence>

        <p className="boot-ritual-ticker" aria-hidden="true">
          {t("bootRitual.ticker")}
        </p>
      </div>
    </div>
  );
}
