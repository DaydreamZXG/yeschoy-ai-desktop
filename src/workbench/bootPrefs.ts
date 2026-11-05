export type BootFrequency = "session" | "daily" | "always";

export const BOOT_KEY = "yeschoy-boot";
export const BOOT_SESSION_KEY = "yeschoy-boot-session-played";

export interface BootPreference {
  enabled: boolean;
  frequency: BootFrequency;
  /** YYYY-MM-DD in local calendar when last played (for daily). */
  lastPlayedDay: string | null;
}

export const DEFAULT_BOOT_PREFERENCE: BootPreference = {
  enabled: true,
  frequency: "daily",
  lastPlayedDay: null,
};

function isFrequency(value: unknown): value is BootFrequency {
  return value === "session" || value === "daily" || value === "always";
}

export function todayKey(now = new Date()): string {
  const y = now.getFullYear();
  const m = String(now.getMonth() + 1).padStart(2, "0");
  const d = String(now.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

export function readBootPreference(): BootPreference {
  try {
    const raw = localStorage.getItem(BOOT_KEY);
    if (!raw) return { ...DEFAULT_BOOT_PREFERENCE };
    const parsed = JSON.parse(raw) as Partial<BootPreference>;
    return {
      enabled:
        typeof parsed.enabled === "boolean"
          ? parsed.enabled
          : DEFAULT_BOOT_PREFERENCE.enabled,
      frequency: isFrequency(parsed.frequency)
        ? parsed.frequency
        : DEFAULT_BOOT_PREFERENCE.frequency,
      lastPlayedDay:
        typeof parsed.lastPlayedDay === "string" ? parsed.lastPlayedDay : null,
    };
  } catch {
    return { ...DEFAULT_BOOT_PREFERENCE };
  }
}

export function writeBootPreference(preference: BootPreference): void {
  try {
    localStorage.setItem(BOOT_KEY, JSON.stringify(preference));
  } catch {
    /* ignore */
  }
}

export function markBootPlayed(
  preference: BootPreference,
  now = new Date(),
): BootPreference {
  const next: BootPreference = {
    ...preference,
    lastPlayedDay: todayKey(now),
  };
  writeBootPreference(next);
  try {
    sessionStorage.setItem(BOOT_SESSION_KEY, "1");
  } catch {
    /* ignore */
  }
  return next;
}

/**
 * Boot ritual only plays on the anime skin. Standard skin never starts it;
 * switching away from anime should close an open ritual in the host.
 * Preferences stay stored so returning to anime can honor frequency again.
 */
export function shouldPlayBoot(
  preference: BootPreference = readBootPreference(),
  now = new Date(),
  skinMode: "standard" | "anime" | null | undefined = undefined,
): boolean {
  if (skinMode !== undefined && skinMode !== null && skinMode !== "anime") {
    return false;
  }
  if (!preference.enabled) return false;
  if (preference.frequency === "always") return true;
  if (preference.frequency === "daily") {
    return preference.lastPlayedDay !== todayKey(now);
  }
  // session
  try {
    return sessionStorage.getItem(BOOT_SESSION_KEY) !== "1";
  } catch {
    return true;
  }
}
