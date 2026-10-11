/**
 * Public desktop download resolution for yeschoy.com/download and embeds.
 * Prefer GitHub Releases when a non-draft release exists; fall back to ergou
 * manifests and permanent installer URLs without breaking in-app ergou updates.
 */

export const DESKTOP_DOWNLOAD_PAGE_URL = "https://yeschoy.com/download";

export const GITHUB_RELEASES_PAGE =
  "https://github.com/DaydreamZXG/yeschoy-ai-desktop/releases";

const GITHUB_RELEASES_API =
  "https://api.github.com/repos/DaydreamZXG/yeschoy-ai-desktop/releases";

export const ERGOU_OFFICIAL_MANIFEST =
  "https://ergou.qzz.io/releases/yeschoy-official.json";

export const ERGOU_PARTNER_MANIFEST =
  "https://ergou.qzz.io/releases/yeschoy-partner.json";

export const ERGOU_PERMANENT_INSTALLERS = {
  official: {
    windows:
      "https://ergou.qzz.io/releases/official/yeschoy-windows-x86_64-installer.exe",
    macos:
      "https://ergou.qzz.io/releases/official/yeschoy-macos-universal-installer.dmg",
  },
  partner: {
    windows:
      "https://ergou.qzz.io/releases/partner/yeschoy-windows-x86_64-installer.exe",
    macos:
      "https://ergou.qzz.io/releases/partner/yeschoy-macos-universal-installer.dmg",
  },
} as const;

export type DownloadVariant = "official" | "partner";
export type DownloadPlatform = "macos" | "windows";

export type DownloadSource = "github" | "ergou";

export interface DesktopInstallerOffer {
  platform: DownloadPlatform;
  url: string;
  source: DownloadSource;
  version: string;
}

export interface DesktopDownloadResolution {
  variant: DownloadVariant;
  detectedPlatform: DownloadPlatform | "other";
  primary: DesktopInstallerOffer | null;
  alternate: Partial<Record<DownloadPlatform, DesktopInstallerOffer>>;
  /** Human-facing note when GitHub has no published release yet. */
  githubUnpublished: boolean;
  releasesPageUrl: string;
}

export function detectDownloadPlatform(
  userAgent = typeof navigator !== "undefined" ? navigator.userAgent : "",
): DownloadPlatform | "other" {
  if (/Windows/i.test(userAgent)) return "windows";
  if (/Macintosh|Mac OS X/i.test(userAgent)) return "macos";
  return "other";
}

export function detectDownloadVariant(
  locationLike: Pick<Location, "hostname" | "search"> = typeof window !==
  "undefined"
    ? window.location
    : { hostname: "", search: "" },
): DownloadVariant {
  const param = new URLSearchParams(locationLike.search).get("variant");
  if (param === "partner") return "partner";
  if (/ai\.yeschoy\.io$/i.test(locationLike.hostname)) return "partner";
  return "official";
}

type GithubRelease = {
  tag_name?: string;
  draft?: boolean;
  published_at?: string | null;
  assets?: { name?: string; browser_download_url?: string }[];
};

type ErgouManifest = {
  version?: string;
  platforms?: Record<string, { url?: string }>;
};

function variantToken(variant: DownloadVariant): string {
  return variant;
}

function pickGithubAsset(
  release: GithubRelease,
  variant: DownloadVariant,
  platform: DownloadPlatform,
): DesktopInstallerOffer | null {
  const version = (release.tag_name ?? "").replace(/^v/i, "");
  if (!version) return null;
  const token = variantToken(variant);
  const assets = release.assets ?? [];
  const match = assets.find((asset) => {
    const name = asset.name ?? "";
    const url = asset.browser_download_url ?? "";
    if (!url.startsWith("https://")) return false;
    if (!name.includes(token)) return false;
    if (platform === "windows") {
      return /windows-x86_64-installer\.exe$/i.test(name);
    }
    return /macos-universal-installer\.dmg$/i.test(name);
  });
  if (!match?.browser_download_url) return null;
  return {
    platform,
    url: match.browser_download_url,
    source: "github",
    version,
  };
}

function pickErgouManifestOffer(
  manifest: ErgouManifest,
  platform: DownloadPlatform,
): DesktopInstallerOffer | null {
  const version = manifest.version ?? "";
  const key = platform === "windows" ? "windows-x86_64" : "macos-universal";
  const url = manifest.platforms?.[key]?.url;
  if (!version || !url || !url.startsWith("https://")) return null;
  return { platform, url, source: "ergou", version };
}

function pickErgouPermanent(
  variant: DownloadVariant,
  platform: DownloadPlatform,
  version: string,
): DesktopInstallerOffer {
  const url = ERGOU_PERMANENT_INSTALLERS[variant][platform];
  return { platform, url, source: "ergou", version };
}

export async function fetchLatestGithubRelease(
  fetchImpl: typeof fetch = fetch,
): Promise<GithubRelease | null> {
  const response = await fetchImpl(GITHUB_RELEASES_API, {
    headers: { Accept: "application/vnd.github+json" },
  });
  if (!response.ok) return null;
  const releases = (await response.json()) as GithubRelease[];
  if (!Array.isArray(releases)) return null;
  return (
    releases.find(
      (item) => item.draft !== true && Boolean(item.published_at?.trim()),
    ) ?? null
  );
}

export async function fetchErgouManifest(
  variant: DownloadVariant,
  fetchImpl: typeof fetch = fetch,
): Promise<ErgouManifest | null> {
  const url =
    variant === "partner"
      ? ERGOU_PARTNER_MANIFEST
      : ERGOU_OFFICIAL_MANIFEST;
  const response = await fetchImpl(url);
  if (!response.ok) return null;
  const manifest = (await response.json()) as ErgouManifest;
  return manifest && typeof manifest === "object" ? manifest : null;
}

/**
 * Resolve installer URLs for both platforms. GitHub wins per platform when the
 * latest published release ships that asset; otherwise ergou manifest, then
 * permanent aliases.
 */
export async function resolveDesktopDownloads(
  options: {
    variant?: DownloadVariant;
    userAgent?: string;
    fetchImpl?: typeof fetch;
  } = {},
): Promise<DesktopDownloadResolution> {
  const variant = options.variant ?? "official";
  const detectedPlatform = detectDownloadPlatform(options.userAgent);
  const fetchImpl = options.fetchImpl ?? fetch;

  const github = await fetchLatestGithubRelease(fetchImpl);
  const ergou = await fetchErgouManifest(variant, fetchImpl);
  const ergouVersion = ergou?.version ?? "";

  const forPlatform = (platform: DownloadPlatform): DesktopInstallerOffer => {
    const fromGithub =
      github && pickGithubAsset(github, variant, platform);
    if (fromGithub) return fromGithub;
    const fromManifest = ergou && pickErgouManifestOffer(ergou, platform);
    if (fromManifest) return fromManifest;
    return pickErgouPermanent(variant, platform, ergouVersion || "latest");
  };

  const alternate: Partial<Record<DownloadPlatform, DesktopInstallerOffer>> =
    {};
  for (const platform of ["macos", "windows"] as const) {
    alternate[platform] = forPlatform(platform);
  }

  const primary =
    detectedPlatform === "other"
      ? null
      : alternate[detectedPlatform] ?? null;

  return {
    variant,
    detectedPlatform,
    primary,
    alternate,
    githubUnpublished: !github,
    releasesPageUrl: GITHUB_RELEASES_PAGE,
  };
}
