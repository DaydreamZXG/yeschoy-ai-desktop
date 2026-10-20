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
const SESSION_REQUIRED_CODES = new Set([
  "session_required",
  "SESSION_REQUIRED",
  "AUTH_UNAUTHORIZED",
  "AUTH_TOKEN_EXPIRED",
  "AUTH_SESSION_REVOKED",
]);

export function normalizeConnectIssueReasonCode(
  code: string | undefined,
  httpStatus?: number,
): string {
  const raw = (code ?? "").trim();
  if (SESSION_REQUIRED_CODES.has(raw) || SESSION_REQUIRED_CODES.has(raw.toUpperCase())) {
    return "session_required";
  }
  if (httpStatus === 401) {
    if (!raw || raw.toUpperCase().startsWith("AUTH_")) {
      return "session_required";
    }
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
  /** Refresh cookie → access token (null on legacy stations without refresh). */
  ensureAccessToken(options?: { fetchImpl?: typeof fetch }): Promise<string | null>;
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

/** Modern New API refresh endpoint (HttpOnly cookie Path=/api/user/auth). */
export const AUTH_REFRESH_PATH = "/api/user/auth/refresh";

let cachedAccessToken: string | null = null;
let cachedAccessExpiresAt = 0;

/** Test helper: clear in-memory access token cache. */
export function resetConnectAccessTokenCache(): void {
  cachedAccessToken = null;
  cachedAccessExpiresAt = 0;
}

function cacheAccessToken(token: string, expiresAt: number): void {
  cachedAccessToken = token;
  cachedAccessExpiresAt = expiresAt > 0 ? expiresAt : 0;
}

function cachedTokenStillValid(): boolean {
  if (!cachedAccessToken) return false;
  if (!cachedAccessExpiresAt) return true;
  return cachedAccessExpiresAt > Math.floor(Date.now() / 1000) + 30;
}

async function parseJsonBody(response: Response): Promise<unknown> {
  try {
    return await response.json();
  } catch {
    return null;
  }
}

/**
 * Exchange the HttpOnly refresh cookie for a short-lived dashboard access token.
 * Returns null when the station has no refresh endpoint (legacy cookie sessions).
 */
export async function ensureAccessToken(
  fetchImpl: typeof fetch = fetch,
): Promise<string | null> {
  if (cachedTokenStillValid()) {
    return cachedAccessToken;
  }

  let response: Response;
  try {
    response = await fetchImpl(AUTH_REFRESH_PATH, {
      method: "POST",
      credentials: "include",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    });
  } catch {
    return null;
  }

  // Older New API builds may not expose refresh — fall back to cookie-only issue.
  if (response.status === 404 || response.status === 405) {
    return null;
  }

  const body = await parseJsonBody(response);
  if (!response.ok || !body || typeof body !== "object" || !("success" in body)) {
    throw new YeschoyConnectError(
      normalizeConnectIssueReasonCode(
        typeof (body as { code?: string })?.code === "string"
          ? (body as { code: string }).code
          : undefined,
        response.status,
      ),
      response.status,
    );
  }

  const envelope = body as { success: boolean; code?: string; data?: unknown };
  if (!envelope.success) {
    throw new YeschoyConnectError(
      normalizeConnectIssueReasonCode(envelope.code, response.status),
      response.status,
    );
  }

  const data = envelope.data;
  if (
    !data ||
    typeof data !== "object" ||
    typeof (data as { access_token?: unknown }).access_token !== "string" ||
    !(data as { access_token: string }).access_token
  ) {
    throw new YeschoyConnectError("issue_invalid_response", response.status);
  }

  const token = (data as { access_token: string }).access_token;
  const expiresAt =
    typeof (data as { access_expires_at?: unknown }).access_expires_at === "number"
      ? (data as { access_expires_at: number }).access_expires_at
      : 0;
  cacheAccessToken(token, expiresAt);
  return token;
}

export async function issueConnectCode(
  fetchImpl: typeof fetch,
  origin: string,
  issuePath: string,
): Promise<IssueConnectCodeResult> {
  const accessToken = await ensureAccessToken(fetchImpl);
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };
  if (accessToken) {
    headers.Authorization = `Bearer ${accessToken}`;
  }

  const response = await fetchImpl(issuePath, {
    method: "POST",
    credentials: "include",
    headers,
    body: JSON.stringify({ origin }),
  });

  const body = await parseJsonBody(response);
  if (!body || typeof body !== "object" || !("success" in body)) {
    throw new YeschoyConnectError(
      normalizeConnectIssueReasonCode(undefined, response.status),
      response.status,
    );
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
    ensureAccessToken: (options = {}) =>
      ensureAccessToken(options.fetchImpl ?? fetch),
    open,
    bindAuto,
  };
}
