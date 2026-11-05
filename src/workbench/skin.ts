import { useLayoutEffect, useState } from "react";

export type SkinMode = "standard" | "anime";
export type SkinCharacter = "dafeiyu" | "claude" | "gpt";

export const SKIN_KEY = "yeschoy-skin";

export const DEFAULT_SKIN_MODE: SkinMode = "anime";
export const DEFAULT_SKIN_CHARACTER: SkinCharacter = "dafeiyu";

export interface SkinPreference {
  mode: SkinMode;
  character: SkinCharacter;
}

function isMode(value: unknown): value is SkinMode {
  return value === "standard" || value === "anime";
}

function isCharacter(value: unknown): value is SkinCharacter {
  return value === "dafeiyu" || value === "claude" || value === "gpt";
}

export function readSkinPreference(): SkinPreference {
  try {
    const raw = localStorage.getItem(SKIN_KEY);
    if (!raw) {
      return { mode: DEFAULT_SKIN_MODE, character: DEFAULT_SKIN_CHARACTER };
    }
    const parsed = JSON.parse(raw) as Partial<SkinPreference>;
    return {
      mode: isMode(parsed.mode) ? parsed.mode : DEFAULT_SKIN_MODE,
      character: isCharacter(parsed.character)
        ? parsed.character
        : DEFAULT_SKIN_CHARACTER,
    };
  } catch {
    return { mode: DEFAULT_SKIN_MODE, character: DEFAULT_SKIN_CHARACTER };
  }
}

export function writeSkinPreference(preference: SkinPreference): void {
  try {
    localStorage.setItem(SKIN_KEY, JSON.stringify(preference));
  } catch {
    /* Session-only fallback. */
  }
}

/** Apply `data-skin` / `data-skin-character` on <html> for CSS hooks. */
export function applySkinToDocument(preference: SkinPreference): void {
  const root = document.documentElement;
  root.dataset.skin = preference.mode;
  if (preference.mode === "anime") {
    root.dataset.skinCharacter = preference.character;
  } else {
    delete root.dataset.skinCharacter;
  }
}

export function useSkin() {
  const [skin, setSkin] = useState<SkinPreference>(readSkinPreference);

  useLayoutEffect(() => {
    applySkinToDocument(skin);
  }, [skin]);

  function changeSkin(next: Partial<SkinPreference>) {
    setSkin((current) => {
      const merged: SkinPreference = {
        mode: next.mode ?? current.mode,
        character: next.character ?? current.character,
      };
      writeSkinPreference(merged);
      return merged;
    });
  }

  return { skin, changeSkin };
}
