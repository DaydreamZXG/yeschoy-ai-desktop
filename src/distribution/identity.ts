/**
 * Compile-time / build-time distribution channel for the desktop UI.
 *
 * Official Yeschoy release pipelines set `VITE_YESCHOY_DISTRIBUTION=official`
 * together with `YESCHOY_DISTRIBUTION=official|partner` (see
 * deploy/self-update/build-local.mjs and src-tauri/build.rs). That pair is the
 * product equivalent of an `OFFICIAL_BUILD` flag: only then is the ergou
 * update channel compiled in and the optional lightweight yeschoy.app link shown.
 *
 * Open-source and default CI builds omit both and ship the OSS-neutral UX
 * (GitHub Releases updater when a community pubkey is provided; no built-in
 * official station as the only default).
 */
export type UiDistribution = "official" | "oss";

export function uiDistribution(): UiDistribution {
  const raw = import.meta.env.VITE_YESCHOY_DISTRIBUTION;
  return raw === "official" ? "official" : "oss";
}

export function isOfficialDistribution(): boolean {
  return uiDistribution() === "official";
}
