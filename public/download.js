var YeschoyDownloadBundle = (function(exports) {
  "use strict";
  const DESKTOP_DOWNLOAD_PAGE_URL = "https://yeschoy.com/download";
  const GITHUB_RELEASES_PAGE = "https://github.com/DaydreamZXG/yeschoy-ai-desktop/releases";
  const GITHUB_RELEASES_API = "https://api.github.com/repos/DaydreamZXG/yeschoy-ai-desktop/releases";
  const ERGOU_OFFICIAL_MANIFEST = "https://ergou.qzz.io/releases/yeschoy-official.json";
  const ERGOU_PARTNER_MANIFEST = "https://ergou.qzz.io/releases/yeschoy-partner.json";
  const ERGOU_PERMANENT_INSTALLERS = {
    official: {
      windows: "https://ergou.qzz.io/releases/official/yeschoy-windows-x86_64-installer.exe",
      macos: "https://ergou.qzz.io/releases/official/yeschoy-macos-universal-installer.dmg"
    },
    partner: {
      windows: "https://ergou.qzz.io/releases/partner/yeschoy-windows-x86_64-installer.exe",
      macos: "https://ergou.qzz.io/releases/partner/yeschoy-macos-universal-installer.dmg"
    }
  };
  function detectDownloadPlatform(userAgent = typeof navigator !== "undefined" ? navigator.userAgent : "") {
    if (/Windows/i.test(userAgent)) return "windows";
    if (/Macintosh|Mac OS X/i.test(userAgent)) return "macos";
    return "other";
  }
  function detectDownloadVariant(locationLike = typeof window !== "undefined" ? window.location : { hostname: "", search: "" }) {
    const param = new URLSearchParams(locationLike.search).get("variant");
    if (param === "partner") return "partner";
    if (/ai\.yeschoy\.io$/i.test(locationLike.hostname)) return "partner";
    return "official";
  }
  function variantToken(variant) {
    return variant;
  }
  function pickGithubAsset(release, variant, platform) {
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
      version
    };
  }
  function pickErgouManifestOffer(manifest, platform) {
    const version = manifest.version ?? "";
    const key = platform === "windows" ? "windows-x86_64" : "macos-universal";
    const url = manifest.platforms?.[key]?.url;
    if (!version || !url || !url.startsWith("https://")) return null;
    return { platform, url, source: "ergou", version };
  }
  function pickErgouPermanent(variant, platform, version) {
    const url = ERGOU_PERMANENT_INSTALLERS[variant][platform];
    return { platform, url, source: "ergou", version };
  }
  async function fetchLatestGithubRelease(fetchImpl = fetch) {
    const response = await fetchImpl(GITHUB_RELEASES_API, {
      headers: { Accept: "application/vnd.github+json" }
    });
    if (!response.ok) return null;
    const releases = await response.json();
    if (!Array.isArray(releases)) return null;
    return releases.find(
      (item) => item.draft !== true && Boolean(item.published_at?.trim())
    ) ?? null;
  }
  async function fetchErgouManifest(variant, fetchImpl = fetch) {
    const url = variant === "partner" ? ERGOU_PARTNER_MANIFEST : ERGOU_OFFICIAL_MANIFEST;
    const response = await fetchImpl(url);
    if (!response.ok) return null;
    const manifest = await response.json();
    return manifest && typeof manifest === "object" ? manifest : null;
  }
  async function resolveDesktopDownloads(options = {}) {
    const variant = options.variant ?? "official";
    const detectedPlatform = detectDownloadPlatform(options.userAgent);
    const fetchImpl = options.fetchImpl ?? fetch;
    const github = await fetchLatestGithubRelease(fetchImpl);
    const ergou = await fetchErgouManifest(variant, fetchImpl);
    const ergouVersion = ergou?.version ?? "";
    const forPlatform = (platform) => {
      const fromGithub = github && pickGithubAsset(github, variant, platform);
      if (fromGithub) return fromGithub;
      const fromManifest = ergou && pickErgouManifestOffer(ergou, platform);
      if (fromManifest) return fromManifest;
      return pickErgouPermanent(variant, platform, ergouVersion || "latest");
    };
    const alternate = {};
    for (const platform of ["macos", "windows"]) {
      alternate[platform] = forPlatform(platform);
    }
    const primary = detectedPlatform === "other" ? null : alternate[detectedPlatform] ?? null;
    return {
      variant,
      detectedPlatform,
      primary,
      alternate,
      githubUnpublished: !github,
      releasesPageUrl: GITHUB_RELEASES_PAGE
    };
  }
  const PLATFORM_LABEL = {
    macos: "下载 Mac 版",
    windows: "下载 Windows 版"
  };
  function platformLabel(platform) {
    return PLATFORM_LABEL[platform];
  }
  function sourceNote(resolution) {
    if (resolution.githubUnpublished) {
      return "GitHub 上还没有公开发布的安装包，当前使用官网备用下载。应用内自动更新仍走原有通道。";
    }
    const primary = resolution.primary;
    if (!primary) return "请选择与你的电脑相符的版本。";
    if (primary.source === "github") {
      return `推荐从 GitHub Releases 下载 v${primary.version}。`;
    }
    return `当前从官网加速下载 v${primary.version}。`;
  }
  function renderButtons(root, resolution) {
    const actions = root.querySelector("[data-download-actions]");
    const note = root.querySelector("[data-download-note]");
    if (!actions || !note) return;
    actions.replaceChildren();
    note.textContent = sourceNote(resolution);
    const platforms = ["macos", "windows"];
    for (const platform of platforms) {
      const offer = resolution.alternate[platform];
      if (!offer) continue;
      const link = document.createElement("a");
      link.className = "download-action";
      link.href = offer.url;
      link.rel = "noopener";
      const isPrimary = resolution.detectedPlatform === platform || resolution.detectedPlatform === "other" && platform === "windows";
      if (isPrimary) link.classList.add("is-primary");
      link.textContent = platformLabel(platform);
      const meta = document.createElement("span");
      meta.className = "download-action-meta";
      meta.textContent = offer.source === "github" ? `v${offer.version} · GitHub` : `v${offer.version} · 官网加速`;
      link.append(meta);
      actions.append(link);
    }
    const releases = root.querySelector(
      "[data-download-releases]"
    );
    if (releases) {
      releases.href = resolution.releasesPageUrl || GITHUB_RELEASES_PAGE;
    }
  }
  async function mountDownloadPage(root) {
    const status = root.querySelector("[data-download-status]");
    if (status) status.textContent = "正在获取下载地址…";
    try {
      const resolution = await resolveDesktopDownloads({
        variant: detectDownloadVariant()
      });
      renderButtons(root, resolution);
      if (status) status.textContent = "";
    } catch {
      if (status) {
        status.textContent = "暂时无法自动匹配下载地址，请稍后再试或打开 GitHub Releases。";
      }
      const releases = root.querySelector(
        "[data-download-releases]"
      );
      if (releases) releases.href = GITHUB_RELEASES_PAGE;
    }
  }
  const api = {
    resolve: resolveDesktopDownloads,
    detectDownloadPlatform,
    detectDownloadVariant,
    mountPage: mountDownloadPage
  };
  window.YeschoyDownload = api;
  function init() {
    const page = document.querySelector("[data-yeschoy-download]");
    if (page) void mountDownloadPage(page);
  }
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
  exports.DESKTOP_DOWNLOAD_PAGE_URL = DESKTOP_DOWNLOAD_PAGE_URL;
  exports.mountDownloadPage = mountDownloadPage;
  Object.defineProperty(exports, Symbol.toStringTag, { value: "Module" });
  return exports;
})({});
