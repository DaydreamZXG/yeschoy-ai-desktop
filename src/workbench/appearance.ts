import { useLayoutEffect, useState } from "react";
import { syncNativeAppearance } from "./nativeAppearance";

export type Appearance = "system" | "light" | "dark";
export const APPEARANCE_KEY = "yeschoy-appearance";
export function readAppearance(): Appearance {
  try {
    const value = localStorage.getItem(APPEARANCE_KEY);
    if (value === "light" || value === "dark") return value;
  } catch {
    /* Storage is optional; never block the UI. */
  }
  return "system";
}
export function useAppearance() {
  const [appearance, setAppearance] = useState<Appearance>(readAppearance);
  useLayoutEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const update = () => {
      const resolved =
        appearance === "system"
          ? media.matches
            ? "dark"
            : "light"
          : appearance;
      document.documentElement.dataset.theme = resolved;
      document.documentElement.style.colorScheme = resolved;
      syncNativeAppearance(resolved);
    };
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, [appearance]);
  function changeAppearance(value: Appearance) {
    setAppearance(value);
    try {
      localStorage.setItem(APPEARANCE_KEY, value);
    } catch {
      /* Session-only fallback. */
    }
  }
  return { appearance, changeAppearance };
}
