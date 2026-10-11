import {
  DESKTOP_DOWNLOAD_PAGE_URL,
  GITHUB_RELEASES_PAGE,
  detectDownloadPlatform,
  detectDownloadVariant,
  resolveDesktopDownloads,
  type DesktopDownloadResolution,
  type DownloadPlatform,
} from "./yeschoyDownload";

declare global {
  interface Window {
    YeschoyDownload?: {
      resolve: typeof resolveDesktopDownloads;
      detectDownloadPlatform: typeof detectDownloadPlatform;
      detectDownloadVariant: typeof detectDownloadVariant;
      mountPage: typeof mountDownloadPage;
    };
  }
}

const PLATFORM_LABEL: Record<DownloadPlatform, string> = {
  macos: "下载 Mac 版",
  windows: "下载 Windows 版",
};

function platformLabel(platform: DownloadPlatform): string {
  return PLATFORM_LABEL[platform];
}

function sourceNote(resolution: DesktopDownloadResolution): string {
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

function renderButtons(
  root: HTMLElement,
  resolution: DesktopDownloadResolution,
): void {
  const actions = root.querySelector<HTMLElement>("[data-download-actions]");
  const note = root.querySelector<HTMLElement>("[data-download-note]");
  if (!actions || !note) return;

  actions.replaceChildren();
  note.textContent = sourceNote(resolution);

  const platforms: DownloadPlatform[] = ["macos", "windows"];
  for (const platform of platforms) {
    const offer = resolution.alternate[platform];
    if (!offer) continue;
    const link = document.createElement("a");
    link.className = "download-action";
    link.href = offer.url;
    link.rel = "noopener";
    const isPrimary =
      resolution.detectedPlatform === platform ||
      (resolution.detectedPlatform === "other" && platform === "windows");
    if (isPrimary) link.classList.add("is-primary");
    link.textContent = platformLabel(platform);
    const meta = document.createElement("span");
    meta.className = "download-action-meta";
    meta.textContent =
      offer.source === "github"
        ? `v${offer.version} · GitHub`
        : `v${offer.version} · 官网加速`;
    link.append(meta);
    actions.append(link);
  }

  const releases = root.querySelector<HTMLAnchorElement>(
    "[data-download-releases]",
  );
  if (releases) {
    releases.href = resolution.releasesPageUrl || GITHUB_RELEASES_PAGE;
  }
}

export async function mountDownloadPage(root: HTMLElement): Promise<void> {
  const status = root.querySelector<HTMLElement>("[data-download-status]");
  if (status) status.textContent = "正在获取下载地址…";
  try {
    const resolution = await resolveDesktopDownloads({
      variant: detectDownloadVariant(),
    });
    renderButtons(root, resolution);
    if (status) status.textContent = "";
  } catch {
    if (status) {
      status.textContent =
        "暂时无法自动匹配下载地址，请稍后再试或打开 GitHub Releases。";
    }
    const releases = root.querySelector<HTMLAnchorElement>(
      "[data-download-releases]",
    );
    if (releases) releases.href = GITHUB_RELEASES_PAGE;
  }
}

const api = {
  resolve: resolveDesktopDownloads,
  detectDownloadPlatform,
  detectDownloadVariant,
  mountPage: mountDownloadPage,
};

window.YeschoyDownload = api;

function init(): void {
  const page = document.querySelector<HTMLElement>("[data-yeschoy-download]");
  if (page) void mountDownloadPage(page);
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", init);
} else {
  init();
}

export { DESKTOP_DOWNLOAD_PAGE_URL };
