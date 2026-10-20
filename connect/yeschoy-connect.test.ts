import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  buildConnectDeepLink,
  ensureAccessToken,
  issueConnectCode,
  navigateDeepLinkWithDownloadFallback,
  normalizeConnectIssueReasonCode,
  pageHttpsOrigin,
  resetConnectAccessTokenCache,
  YeschoyConnectError,
} from "./yeschoy-connect";

beforeEach(() => {
  resetConnectAccessTokenCache();
});

describe("pageHttpsOrigin", () => {
  it("accepts https origins", () => {
    expect(
      pageHttpsOrigin({ protocol: "https:", origin: "https://yeschoy.com" }),
    ).toBe("https://yeschoy.com");
  });

  it("rejects http", () => {
    expect(
      pageHttpsOrigin({ protocol: "http:", origin: "http://yeschoy.com" }),
    ).toBeNull();
  });
});

describe("buildConnectDeepLink", () => {
  it("encodes origin and code without tokens in query names", () => {
    const url = buildConnectDeepLink("https://example.com", "abc_def-123");
    expect(url).toBe(
      "yeschoy://connect?origin=https%3A%2F%2Fexample.com&code=abc_def-123",
    );
    expect(url).not.toMatch(/access_token|refresh_token|api_key/i);
  });
});

describe("normalizeConnectIssueReasonCode", () => {
  it("maps AUTH_UNAUTHORIZED and 401 to session_required", () => {
    expect(normalizeConnectIssueReasonCode("AUTH_UNAUTHORIZED", 401)).toBe(
      "session_required",
    );
    expect(normalizeConnectIssueReasonCode("session_required", 401)).toBe(
      "session_required",
    );
    expect(normalizeConnectIssueReasonCode(undefined, 401)).toBe(
      "session_required",
    );
  });

  it("preserves other documented issue failures", () => {
    expect(normalizeConnectIssueReasonCode("connect_origin_invalid", 400)).toBe(
      "connect_origin_invalid",
    );
    expect(normalizeConnectIssueReasonCode(undefined, 500)).toBe("issue_failed");
  });
});

describe("ensureAccessToken", () => {
  it("POSTs /api/user/auth/refresh and returns access_token", async () => {
    const fetchImpl = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        success: true,
        data: {
          access_token: "access-from-refresh",
          access_expires_at: Math.floor(Date.now() / 1000) + 600,
        },
      }),
    });
    await expect(ensureAccessToken(fetchImpl)).resolves.toBe("access-from-refresh");
    expect(fetchImpl).toHaveBeenCalledWith("/api/user/auth/refresh", expect.objectContaining({
      method: "POST",
      credentials: "include",
    }));
  });

  it("returns null when refresh endpoint is missing (legacy)", async () => {
    const fetchImpl = vi.fn().mockResolvedValue({
      ok: false,
      status: 404,
      json: async () => ({ success: false }),
    });
    await expect(ensureAccessToken(fetchImpl)).resolves.toBeNull();
  });
});

describe("issueConnectCode", () => {
  it("refreshes then posts Bearer on modern New API", async () => {
    const fetchImpl = vi.fn(async (url: string) => {
      if (String(url) === "/api/user/auth/refresh") {
        return {
          ok: true,
          status: 200,
          json: async () => ({
            success: true,
            data: { access_token: "bearer-token", access_expires_at: Math.floor(Date.now() / 1000) + 600 },
          }),
        };
      }
      return {
        ok: true,
        status: 200,
        json: async () => ({
          success: true,
          data: { code: "c1", origin: "https://station.test" },
        }),
      };
    });

    const result = await issueConnectCode(
      fetchImpl as unknown as typeof fetch,
      "https://station.test",
      "/api/desktop/v2/connect-codes",
    );

    expect(result).toEqual({ code: "c1", origin: "https://station.test" });
    expect(fetchImpl).toHaveBeenNthCalledWith(1, "/api/user/auth/refresh", expect.any(Object));
    expect(fetchImpl).toHaveBeenNthCalledWith(2, "/api/desktop/v2/connect-codes", {
      method: "POST",
      credentials: "include",
      headers: {
        "Content-Type": "application/json",
        Authorization: "Bearer bearer-token",
      },
      body: JSON.stringify({ origin: "https://station.test" }),
    });
  });

  it("falls back to cookie-only issue when refresh is 404", async () => {
    const fetchImpl = vi.fn(async (url: string) => {
      if (String(url) === "/api/user/auth/refresh") {
        return { ok: false, status: 404, json: async () => ({}) };
      }
      return {
        ok: true,
        status: 200,
        json: async () => ({
          success: true,
          data: { code: "legacy", origin: "https://station.test" },
        }),
      };
    });

    const result = await issueConnectCode(
      fetchImpl as unknown as typeof fetch,
      "https://station.test",
      "/api/desktop/v2/connect-codes",
    );
    expect(result.code).toBe("legacy");
    expect(fetchImpl).toHaveBeenNthCalledWith(2, "/api/desktop/v2/connect-codes", {
      method: "POST",
      credentials: "include",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ origin: "https://station.test" }),
    });
  });

  it("maps refresh 401 to session_required", async () => {
    const fetchImpl = vi.fn().mockResolvedValue({
      ok: false,
      status: 401,
      json: async () => ({ success: false, code: "AUTH_UNAUTHORIZED" }),
    });

    await expect(
      issueConnectCode(fetchImpl, "https://station.test", "/api/desktop/v2/connect-codes"),
    ).rejects.toMatchObject({
      reasonCode: "session_required",
      httpStatus: 401,
    });
  });

  it("maps issue API errors to YeschoyConnectError after successful refresh", async () => {
    const fetchImpl = vi.fn(async (url: string) => {
      if (String(url) === "/api/user/auth/refresh") {
        return {
          ok: true,
          status: 200,
          json: async () => ({
            success: true,
            data: { access_token: "t", access_expires_at: Math.floor(Date.now() / 1000) + 600 },
          }),
        };
      }
      return {
        ok: false,
        status: 401,
        json: async () => ({ success: false, code: "session_required" }),
      };
    });

    await expect(
      issueConnectCode(fetchImpl as unknown as typeof fetch, "https://station.test", "/api/desktop/v2/connect-codes"),
    ).rejects.toMatchObject({
      reasonCode: "session_required",
      httpStatus: 401,
    });
  });
});

describe("navigateDeepLinkWithDownloadFallback", () => {
  it("opens deep link immediately", () => {
    const location = { href: "", assign: vi.fn() };
    const win = {
      location,
      addEventListener: vi.fn(),
      setTimeout: vi.fn(() => 1),
      clearTimeout: vi.fn(),
    };

    navigateDeepLinkWithDownloadFallback(
      "yeschoy://connect?origin=https%3A%2F%2Fa.com&code=x",
      "https://yeschoy.com/download",
      win,
      { missingAppTimeoutMs: 2000 },
    );

    expect(location.href).toContain("yeschoy://connect");
  });

  it("falls back to download when blur never fires", () => {
    const assign = vi.fn();
    const location = { href: "", assign };
    let timeoutCb: (() => void) | undefined;
    const win = {
      location,
      addEventListener: vi.fn(),
      setTimeout: vi.fn((cb: () => void) => {
        timeoutCb = cb;
        return 1;
      }),
      clearTimeout: vi.fn(),
    };

    navigateDeepLinkWithDownloadFallback(
      "yeschoy://connect?origin=https%3A%2F%2Fa.com&code=x",
      "https://yeschoy.com/download",
      win,
      { missingAppTimeoutMs: 100 },
    );

    timeoutCb?.();
    expect(assign).toHaveBeenCalledWith("https://yeschoy.com/download");
    expect(assign.mock.calls[0][0]).not.toContain("ergou");
  });

  it("cancels fallback on blur", () => {
    const assign = vi.fn();
    const location = { href: "", assign };
    let blurHandler: (() => void) | undefined;
    let timeoutCb: (() => void) | undefined;
    const win = {
      location,
      addEventListener: vi.fn((_ev: string, handler: () => void) => {
        blurHandler = handler;
      }),
      setTimeout: vi.fn((cb: () => void) => {
        timeoutCb = cb;
        return 1;
      }),
      clearTimeout: vi.fn(),
    };

    navigateDeepLinkWithDownloadFallback(
      "yeschoy://connect?origin=https%3A%2F%2Fa.com&code=x",
      "https://yeschoy.com/download",
      win,
      { missingAppTimeoutMs: 100 },
    );

    blurHandler?.();
    timeoutCb?.();
    expect(assign).not.toHaveBeenCalled();
  });
});

describe("YeschoyConnectError", () => {
  it("exposes reasonCode", () => {
    const err = new YeschoyConnectError("session_required", 401);
    expect(err.reasonCode).toBe("session_required");
    expect(err.httpStatus).toBe(401);
  });
});
