/**
 * Compile-time / build-time distribution channel for the desktop UI.
 *
 * Official Yeschoy release pipelines set `VITE_YESCHOY_DISTRIBUTION=official`
 * (see deploy/self-update/build-local.mjs). Open-source and default CI builds
 * omit it and ship the OSS-neutral UX.
 */
export type UiDistribution = "official" | "oss";

export function uiDistribution(): UiDistribution {
  const raw = import.meta.env.VITE_YESCHOY_DISTRIBUTION;
  return raw === "official" ? "official" : "oss";
}

export function isOfficialDistribution(): boolean {
  return uiDistribution() === "official";
}
