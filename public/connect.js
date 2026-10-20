(function() {
  "use strict";
  const CONNECT_CODES_ISSUE_PATH = "/api/desktop/v2/connect-codes";
  const DEFAULT_DOWNLOAD_URL = "https://yeschoy.com/download";
  class YeschoyConnectError extends Error {
    constructor(reasonCode, httpStatus) {
      super(reasonCode);
      this.name = "YeschoyConnectError";
      this.reasonCode = reasonCode;
      this.httpStatus = httpStatus;
    }
  }
  const SESSION_REQUIRED_CODES = /* @__PURE__ */ new Set([
    "session_required",
    "SESSION_REQUIRED",
    "AUTH_UNAUTHORIZED",
    "AUTH_TOKEN_EXPIRED",
    "AUTH_SESSION_REVOKED"
  ]);
  function normalizeConnectIssueReasonCode(code, httpStatus) {
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
  function pageHttpsOrigin(location) {
    if (location.protocol !== "https:") {
      return null;
    }
    return location.origin;
  }
  function buildConnectDeepLink(origin, code) {
    const params = new URLSearchParams();
    params.set("origin", origin);
    params.set("code", code);
    return `yeschoy://connect?${params.toString()}`;
  }
  const AUTH_REFRESH_PATH = "/api/user/auth/refresh";
  let cachedAccessToken = null;
  let cachedAccessExpiresAt = 0;
  function cacheAccessToken(token, expiresAt) {
    cachedAccessToken = token;
    cachedAccessExpiresAt = expiresAt > 0 ? expiresAt : 0;
  }
  function cachedTokenStillValid() {
    if (!cachedAccessToken) return false;
    if (!cachedAccessExpiresAt) return true;
    return cachedAccessExpiresAt > Math.floor(Date.now() / 1e3) + 30;
  }
  async function parseJsonBody(response) {
    try {
      return await response.json();
    } catch {
      return null;
    }
  }
  async function ensureAccessToken(fetchImpl = fetch) {
    if (cachedTokenStillValid()) {
      return cachedAccessToken;
    }
    let response;
    try {
      response = await fetchImpl(AUTH_REFRESH_PATH, {
        method: "POST",
        credentials: "include",
        headers: { "Content-Type": "application/json" },
        body: "{}"
      });
    } catch {
      return null;
    }
    if (response.status === 404 || response.status === 405) {
      return null;
    }
    const body = await parseJsonBody(response);
    if (!response.ok || !body || typeof body !== "object" || !("success" in body)) {
      throw new YeschoyConnectError(
        normalizeConnectIssueReasonCode(
          typeof body?.code === "string" ? body.code : void 0,
          response.status
        ),
        response.status
      );
    }
    const envelope = body;
    if (!envelope.success) {
      throw new YeschoyConnectError(
        normalizeConnectIssueReasonCode(envelope.code, response.status),
        response.status
      );
    }
    const data = envelope.data;
    if (!data || typeof data !== "object" || typeof data.access_token !== "string" || !data.access_token) {
      throw new YeschoyConnectError("issue_invalid_response", response.status);
    }
    const token = data.access_token;
    const expiresAt = typeof data.access_expires_at === "number" ? data.access_expires_at : 0;
    cacheAccessToken(token, expiresAt);
    return token;
  }
  async function issueConnectCode(fetchImpl, origin, issuePath) {
    const accessToken = await ensureAccessToken(fetchImpl);
    const headers = {
      "Content-Type": "application/json"
    };
    if (accessToken) {
      headers.Authorization = `Bearer ${accessToken}`;
    }
    const response = await fetchImpl(issuePath, {
      method: "POST",
      credentials: "include",
      headers,
      body: JSON.stringify({ origin })
    });
    const body = await parseJsonBody(response);
    if (!body || typeof body !== "object" || !("success" in body)) {
      throw new YeschoyConnectError(
        normalizeConnectIssueReasonCode(void 0, response.status),
        response.status
      );
    }
    const envelope = body;
    if (!envelope.success) {
      throw new YeschoyConnectError(
        normalizeConnectIssueReasonCode(envelope.code, response.status),
        response.status
      );
    }
    const data = envelope.data;
    if (!data || typeof data !== "object") {
      throw new YeschoyConnectError("issue_invalid_response", response.status);
    }
    const record = data;
    if (typeof record.code !== "string" || typeof record.origin !== "string") {
      throw new YeschoyConnectError("issue_invalid_response", response.status);
    }
    return { code: record.code, origin: record.origin };
  }
  function navigateDeepLinkWithDownloadFallback(deepLink, downloadUrl, win, options) {
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
      win.removeEventListener?.("blur", onBlur);
    };
    win.addEventListener("blur", onBlur);
    options.onOpening?.();
    win.location.href = deepLink;
  }
  function createYeschoyConnect(defaults = {}) {
    const issuePath = defaults.issuePath ?? CONNECT_CODES_ISSUE_PATH;
    const downloadUrl = defaults.downloadUrl ?? DEFAULT_DOWNLOAD_URL;
    const missingAppTimeoutMs = defaults.missingAppTimeoutMs ?? 2e3;
    const getOrigin = defaults.getOrigin ?? (() => typeof window !== "undefined" ? pageHttpsOrigin(window.location) : null);
    const dispatchPhase = defaults.dispatchPhase ?? (() => {
    });
    async function open(options = {}) {
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
        onFallbackDownload: () => dispatchPhase("redirect_download")
      });
    }
    function bindAuto(root = document) {
      const nodes = root.querySelectorAll("[data-yeschoy-connect]");
      nodes.forEach((el) => {
        if (el.dataset.yeschoyConnectBound === "1") return;
        el.dataset.yeschoyConnectBound = "1";
        el.addEventListener("click", (event) => {
          event.preventDefault();
          void open().catch((err) => {
            el.dispatchEvent(
              new CustomEvent("yeschoy-connect-error", {
                detail: err,
                bubbles: true
              })
            );
          });
        });
      });
    }
    return {
      issuePath,
      downloadUrl,
      ensureAccessToken: (options = {}) => ensureAccessToken(options.fetchImpl ?? fetch),
      open,
      bindAuto
    };
  }
  const scriptEl = document.currentScript;
  const downloadOverride = scriptEl instanceof HTMLScriptElement ? scriptEl.dataset.downloadUrl?.trim() : void 0;
  const api = createYeschoyConnect({
    downloadUrl: downloadOverride || void 0
  });
  window.YeschoyConnect = api;
  function shouldAutoLaunchFromDesktop() {
    const params = new URLSearchParams(window.location.search);
    return params.get("auto") === "1";
  }
  function init() {
    api.bindAuto();
    if (!shouldAutoLaunchFromDesktop()) return;
    const button = document.querySelector("[data-yeschoy-connect]");
    if (button) {
      button.hidden = true;
      button.setAttribute("aria-hidden", "true");
    }
    document.dispatchEvent(new CustomEvent("yeschoy-connect-auto-start"));
    void api.open().catch((err) => {
      if (button) {
        button.hidden = false;
        button.removeAttribute("aria-hidden");
      }
      button?.dispatchEvent(
        new CustomEvent("yeschoy-connect-error", {
          detail: err,
          bubbles: true
        })
      );
    });
  }
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
