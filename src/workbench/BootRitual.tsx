import { useCallback, useEffect, useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { useTranslation } from "react-i18next";
import type { SkinCharacter } from "./skin";
import { markBootPlayed, readBootPreference } from "./bootPrefs";
import {
  BootMotesSvg,
  BootRefractSvg,
  BootSigilSvg,
  BootVeilSvg,
} from "./bootSvg";
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

/** AMV beat pacing. Stage 2 waits on 切入, then auto-advances. ~7s full run. */
const STAGE_MS: Record<BootStage, number> = {
  1: 1200,
  2: 2800,
  3: 1200,
  4: 1500,
};

const LEAVE_MS = 360;
const IMPACT_MS = 160;
const REFRACT_MS = 280;

const STAGE_COPY: Record<BootStage, { title: string; en: string }> = {
  1: { title: "bootRitual.stage1Title", en: "bootRitual.stage1En" },
  2: { title: "bootRitual.stage2Title", en: "bootRitual.stage2En" },
  3: { title: "bootRitual.stage3Title", en: "bootRitual.stage3En" },
  4: { title: "bootRitual.stage4Title", en: "bootRitual.stage4En" },
};

const hardCut = (reduce: boolean) =>
  reduce
    ? {
        initial: { opacity: 1 },
        animate: { opacity: 1 },
        exit: { opacity: 0 },
        transition: { duration: 0.08 },
      }
    : {
        initial: { opacity: 1 },
        animate: { opacity: 1 },
        exit: { opacity: 1 },
        transition: { duration: 0 },
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
  const [cutting, setCutting] = useState(!reduceMotion);
  const finished = useRef(false);
  const flashTimer = useRef<number | null>(null);
  const cutTimer = useRef<number | null>(null);

  const punchCut = useCallback(() => {
    if (reduceMotion) return;
    setCutting(true);
    if (cutTimer.current != null) window.clearTimeout(cutTimer.current);
    cutTimer.current = window.setTimeout(() => {
      setCutting(false);
      cutTimer.current = null;
    }, IMPACT_MS);
  }, [reduceMotion]);

  const finish = useCallback(() => {
    if (finished.current) return;
    finished.current = true;
    if (flashTimer.current != null) {
      window.clearTimeout(flashTimer.current);
      flashTimer.current = null;
    }
    if (cutTimer.current != null) {
      window.clearTimeout(cutTimer.current);
      cutTimer.current = null;
    }
    setFlashing(false);
    setCutting(false);
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

  const ignite = useCallback(() => {
    if (stage !== 2 || finished.current || flashing) return;
    setFlashing(true);
    if (flashTimer.current != null) window.clearTimeout(flashTimer.current);
    flashTimer.current = window.setTimeout(
      () => {
        setFlashing(false);
        flashTimer.current = null;
        advanceFrom(2);
      },
      reduceMotion ? 80 : REFRACT_MS,
    );
  }, [stage, flashing, advanceFrom, reduceMotion]);

  useEffect(() => {
    punchCut();
  }, [stage, punchCut]);

  useEffect(() => {
    if (disableAutoAdvance) return;
    if (stage === 2) {
      if (flashing) return;
      const ms = reduceMotion ? 1000 : STAGE_MS[2];
      const id = window.setTimeout(() => ignite(), ms);
      return () => window.clearTimeout(id);
    }
    const ms = STAGE_MS[stage];
    const id = window.setTimeout(() => advanceFrom(stage), ms);
    return () => window.clearTimeout(id);
  }, [stage, advanceFrom, disableAutoAdvance, flashing, ignite, reduceMotion]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        finish();
        return;
      }
      if (stage === 2 && (event.key === "Enter" || event.key === " ")) {
        event.preventDefault();
        ignite();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [stage, finish, ignite]);

  useEffect(() => {
    return () => {
      if (flashTimer.current != null) window.clearTimeout(flashTimer.current);
      if (cutTimer.current != null) window.clearTimeout(cutTimer.current);
    };
  }, []);

  const copy = STAGE_COPY[stage];
  const motionProps = hardCut(reduceMotion);
  const title = t(copy.title);

  return (
    <div
      className="boot-ritual"
      data-testid="boot-ritual"
      data-visual="flashcut"
      data-stage={stage}
      data-character={character}
      data-leaving={leaving ? "true" : "false"}
      data-flashing={flashing ? "true" : "false"}
      data-cutting={cutting ? "true" : "false"}
      role="dialog"
      aria-modal="true"
      aria-labelledby="boot-ritual-title"
    >
      <div className="boot-ritual-wallpaper" aria-hidden="true" />
      <div className="boot-ritual-grade" aria-hidden="true" />
      <div className="boot-ritual-beams" aria-hidden="true" />
      <div className="boot-ritual-streaks" aria-hidden="true" />
      <div className="boot-ritual-chroma" aria-hidden="true" />
      <BootVeilSvg />
      <BootMotesSvg />
      <BootRefractSvg active={flashing} />
      <div className="boot-ritual-impact" aria-hidden="true" />

      <div className="boot-ritual-letterbox" aria-hidden="true">
        <span className="boot-ritual-letterbox-top" />
        <span className="boot-ritual-letterbox-bottom" />
      </div>

      <button
        type="button"
        className="boot-ritual-skip"
        data-testid="boot-ritual-skip"
        onClick={finish}
      >
        {t("bootRitual.skip")}
      </button>

      <div className="boot-ritual-stage">
        <div className="boot-ritual-sigil" aria-hidden="true">
          <BootSigilSvg />
        </div>

        <p className="boot-ritual-index" aria-hidden="true">
          {String(stage).padStart(2, "0")}
          <span> / 04</span>
        </p>

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

        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={stage}
            className="boot-ritual-act"
            data-testid="boot-ritual-act"
            initial={motionProps.initial}
            animate={motionProps.animate}
            exit={motionProps.exit}
            transition={motionProps.transition}
          >
            <p className="boot-ritual-kicker" aria-hidden="true">
              FLASHCUT
            </p>
            <h1
              id="boot-ritual-title"
              className="boot-ritual-title"
              data-text={title}
            >
              {title}
            </h1>
            <p className="boot-ritual-en" lang="en">
              {t(copy.en)}
            </p>

            {stage === 2 && (
              <button
                type="button"
                className="boot-ritual-ignite"
                data-testid="boot-ritual-breach"
                onClick={ignite}
              >
                <span>{t("bootRitual.breach")}</span>
              </button>
            )}
          </motion.div>
        </AnimatePresence>

        <p className="boot-ritual-mark" aria-hidden="true">
          {t("bootRitual.ticker")}
        </p>
      </div>
    </div>
  );
}
