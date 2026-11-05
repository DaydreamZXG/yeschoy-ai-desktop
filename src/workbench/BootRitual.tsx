import { useCallback, useEffect, useRef, useState } from "react";
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

const STAGE_MS: Record<BootStage, number> = {
  1: 1400,
  2: 0, // waits for BREACH
  3: 1200,
  4: 1100,
};

export function BootRitual({
  character,
  onComplete,
  initialStage = 1,
  disableAutoAdvance = false,
}: BootRitualProps) {
  const { t } = useTranslation();
  const [stage, setStage] = useState<BootStage>(initialStage);
  const [leaving, setLeaving] = useState(false);
  const finished = useRef(false);

  const finish = useCallback(() => {
    if (finished.current) return;
    finished.current = true;
    markBootPlayed(readBootPreference());
    setLeaving(true);
    window.setTimeout(() => onComplete(), 280);
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

  useEffect(() => {
    if (disableAutoAdvance) return;
    if (stage === 2) return;
    const ms = STAGE_MS[stage];
    const id = window.setTimeout(() => advanceFrom(stage), ms);
    return () => window.clearTimeout(id);
  }, [stage, advanceFrom, disableAutoAdvance]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        finish();
        return;
      }
      if (stage === 2 && (event.key === "Enter" || event.key === " ")) {
        event.preventDefault();
        advanceFrom(2);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [stage, finish, advanceFrom]);

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

  return (
    <div
      className="boot-ritual"
      data-testid="boot-ritual"
      data-stage={stage}
      data-character={character}
      data-leaving={leaving ? "true" : "false"}
      role="dialog"
      aria-modal="true"
      aria-labelledby="boot-ritual-title"
    >
      <div className="boot-ritual-wallpaper" aria-hidden="true" />
      <div className="boot-ritual-glass" aria-hidden="true" />

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
          <span />
          <span />
        </div>
        <p className="boot-ritual-index" aria-hidden="true">
          {String(stage).padStart(2, "0")} / 04
        </p>
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
            onClick={() => advanceFrom(2)}
          >
            {t("bootRitual.breach")}
          </button>
        )}

        {stage === 3 && (
          <div className="boot-ritual-prism" aria-hidden="true" />
        )}

        <p className="boot-ritual-ticker" aria-hidden="true">
          {t("bootRitual.ticker")}
        </p>
      </div>
    </div>
  );
}
