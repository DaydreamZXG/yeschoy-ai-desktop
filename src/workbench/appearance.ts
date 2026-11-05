import { useLayoutEffect, useState } from "react";
import { syncNativeAppearance } from "./nativeAppearance";

export type Appearance = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";
export const APPEARANCE_KEY = "yeschoy-appearance";

export function readAppearance(): Appearance {
  try {
    const value =
      localStorage.getItem(APPEARANCE_KEY) ||
      localStorage.getItem("cc-switch-theme");
    if (value === "light" || value === "dark") return value;
  } catch {
    /* Storage is optional; never block the UI. */
  }
  return "system";
}

export function resolveTheme(
  appearance: Appearance,
  prefersDark: boolean,
  options?: { lockLight?: boolean },
): ResolvedTheme {
  if (options?.lockLight) return "light";
  if (appearance === "system") return prefersDark ? "dark" : "light";
  return appearance;
}

export function useAppearance(options?: { lockLight?: boolean }) {
  const lockLight = options?.lockLight === true;
  const [appearance, setAppearance] = useState<Appearance>(readAppearance);
  const [resolvedTheme, setResolvedTheme] = useState<ResolvedTheme>("light");
  useLayoutEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const update = () => {
      const resolved = resolveTheme(appearance, media.matches, { lockLight });
      setResolvedTheme(resolved);
      document.documentElement.dataset.theme = resolved;
      document.documentElement.style.colorScheme = resolved;
      syncNativeAppearance(resolved);
    };
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, [appearance, lockLight]);
  function changeAppearance(value: Appearance) {
    if (lockLight) return;
    setAppearance(value);
    try {
      localStorage.setItem(APPEARANCE_KEY, value);
    } catch {
      /* Session-only fallback. */
    }
  }
  return { appearance, changeAppearance, resolvedTheme };
}
