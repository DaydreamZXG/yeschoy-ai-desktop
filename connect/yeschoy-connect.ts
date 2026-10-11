/**
 * YesChoy station connect — browser-side issue + deep-link navigation.
 * Bundled to `public/connect.js` for https://yeschoy.com/connect.js
 */

export const CONNECT_CODES_ISSUE_PATH = "/api/desktop/v2/connect-codes";

/** Documented station-hosted connect UI path (see docs/station-connect-page.md). */
export { STATION_CONNECT_PAGE_PATH } from "./stationConnectPage";

/** Public download page (not ergou update CDN). */
export const DEFAULT_DOWNLOAD_URL = "https://yeschoy.com/download";

export const GITHUB_RELEASES_URL =
  "https://github.com/DaydreamZXG/yeschoy-ai-desktop/releases";

export type YeschoyConnectPhase =
  | "idle"
  | "issuing"
  | "opening"
  | "redirect_download";

export class YeschoyConnectError extends Error {
  readonly reasonCode: string;
  readonly httpStatus?: number;

  constructor(reasonCode: string, httpStatus?: number) {
    super(reasonCode);
    this.name = "YeschoyConnectError";
    this.reasonCode = reasonCode;
    this.httpStatus = httpStatus;
  }
}

/** Map station issue API codes to stable UI reason codes (see station-connect OpenAPI). */
export function normalizeConnectIssueReasonCode(
  code: string | undefined,
  httpStatus?: number,
): string {
  const raw = (code ?? "").trim();
  const upper = raw.toUpperCase();
  if (upper === "SESSION_REQUIRED" || upper === "AUTH_UNAUTHORIZED") {
    return "session_required";
  }
  if (httpStatus === 401) {
    return "session_required";
  }
  return raw || "issue_failed";
}

export interface YeschoyConnectOptions {
  /** Canonical HTTPS origin; defaults to current page origin. */
  origin?: string;
  /** Same-origin issue path. */
  issuePath?: string;
  /** Where to send users when the desktop app is likely not installed. */
  downloadUrl?: string;
  /** Ms to wait before download fallback after launching the deep link. */
  missingAppTimeoutMs?: number;
  fetchImpl?: typeof fetch;
}

export interface YeschoyConnectApi {
  readonly issuePath: string;
  readonly downloadUrl: string;
  open(options?: YeschoyConnectOptions): Promise<void>;
  bindAuto(root?: ParentNode): void;
}

export function pageHttpsOrigin(location: Pick<Location, "protocol" | "origin">): string | null {
  if (location.protocol !== "https:") {
    return null;
  }
  return location.origin;
}

export function buildConnectDeepLink(origin: string, code: string): string {
  const params = new URLSearchParams();
  params.set("origin", origin);
  params.set("code", code);
  return `yeschoy://connect?${params.toString()}`;
}

export interface IssueConnectCodeResult {
  code: string;
  origin: string;
}

export async function issueConnectCode(
  fetchImpl: typeof fetch,
  origin: string,
  issuePath: string,
): Promise<IssueConnectCodeResult> {
  const response = await fetchImpl(issuePath, {
    method: "POST",
    credentials: "include",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ origin }),
  });

  let body: unknown;
  try {
    body = await response.json();
  } catch {
    throw new YeschoyConnectError("issue_invalid_response", response.status);
  }

  if (!body || typeof body !== "object" || !("success" in body)) {
    throw new YeschoyConnectError("issue_invalid_response", response.status);
  }

  const envelope = body as { success: boolean; code?: string; data?: unknown };
  if (!envelope.success) {
    throw new YeschoyConnectError(
      normalizeConnectIssueReasonCode(envelope.code, response.status),
      response.status,
    );
  }

  const data = envelope.data;
  if (!data || typeof data !== "object") {
    throw new YeschoyConnectError("issue_invalid_response", response.status);
  }

  const record = data as { code?: unknown; origin?: unknown };
  if (typeof record.code !== "string" || typeof record.origin !== "string") {
    throw new YeschoyConnectError("issue_invalid_response", response.status);
  }

  return { code: record.code, origin: record.origin };
}

export interface NavigateDeepLinkOptions {
  missingAppTimeoutMs: number;
  onOpening?: () => void;
  onFallbackDownload?: () => void;
}

/**
 * Navigate to a custom-scheme deep link; if the page stays focused, open the download URL.
 */
export function navigateDeepLinkWithDownloadFallback(
  deepLink: string,
  downloadUrl: string,
  win: Pick<Window, "location" | "addEventListener" | "setTimeout" | "clearTimeout">,
  options: NavigateDeepLinkOptions,
): void {
  let fallbackScheduled = true;
  const clearFallback = () => {
    fallbackScheduled = false;
  };

  const timer = win.setTimeout(() => {
    if (!fallbackScheduled) return;
    options.onFallbackDownload?.();
    win.location.assign(downloadUrl);
  }, options.missingAppTimeoutMs);

  const onBlur = () => {
    win.clearTimeout(timer);
    clearFallback();
    win.removeEventListener?.("blur", onBlur as EventListener);
  };

  win.addEventListener("blur", onBlur as EventListener);
  options.onOpening?.();
  win.location.href = deepLink;
}

export function createYeschoyConnect(
  defaults: {
    issuePath?: string;
    downloadUrl?: string;
    missingAppTimeoutMs?: number;
    getOrigin?: () => string | null;
    dispatchPhase?: (phase: YeschoyConnectPhase) => void;
  } = {},
): YeschoyConnectApi {
  const issuePath = defaults.issuePath ?? CONNECT_CODES_ISSUE_PATH;
  const downloadUrl = defaults.downloadUrl ?? DEFAULT_DOWNLOAD_URL;
  const missingAppTimeoutMs = defaults.missingAppTimeoutMs ?? 2000;
  const getOrigin =
    defaults.getOrigin ??
    (() =>
      typeof window !== "undefined"
        ? pageHttpsOrigin(window.location)
        : null);
  const dispatchPhase = defaults.dispatchPhase ?? (() => {});

  async function open(options: YeschoyConnectOptions = {}): Promise<void> {
    const fetchImpl = options.fetchImpl ?? fetch;
    const origin = options.origin ?? getOrigin();
    if (!origin) {
      throw new YeschoyConnectError("connect_origin_not_https");
    }

    const path = options.issuePath ?? issuePath;
    const fallbackUrl = options.downloadUrl ?? downloadUrl;
    const timeoutMs = options.missingAppTimeoutMs ?? missingAppTimeoutMs;

    dispatchPhase("issuing");
    const issued = await issueConnectCode(fetchImpl, origin, path);
    const deepLink = buildConnectDeepLink(issued.origin, issued.code);

    if (typeof window === "undefined") {
      throw new YeschoyConnectError("browser_only");
    }

    navigateDeepLinkWithDownloadFallback(deepLink, fallbackUrl, window, {
      missingAppTimeoutMs: timeoutMs,
      onOpening: () => dispatchPhase("opening"),
      onFallbackDownload: () => dispatchPhase("redirect_download"),
    });
  }

  function bindAuto(root: ParentNode = document): void {
    const nodes = root.querySelectorAll<HTMLElement>("[data-yeschoy-connect]");
    nodes.forEach((el) => {
      if (el.dataset.yeschoyConnectBound === "1") return;
      el.dataset.yeschoyConnectBound = "1";
      el.addEventListener("click", (event) => {
        event.preventDefault();
        void open().catch((err) => {
          el.dispatchEvent(
            new CustomEvent("yeschoy-connect-error", {
              detail: err,
              bubbles: true,
            }),
          );
        });
      });
    });
  }

  return {
    issuePath,
    downloadUrl,
    open,
    bindAuto,
  };
}
