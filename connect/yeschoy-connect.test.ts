import { describe, expect, it, vi } from "vitest";
import {
  buildConnectDeepLink,
  issueConnectCode,
  navigateDeepLinkWithDownloadFallback,
  pageHttpsOrigin,
  YeschoyConnectError,
} from "./yeschoy-connect";

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

describe("issueConnectCode", () => {
  it("posts same-origin JSON with credentials", async () => {
    const fetchImpl = vi.fn().mockResolvedValue({
      status: 200,
      json: async () => ({
        success: true,
        data: { code: "c1", origin: "https://station.test" },
      }),
    });

    const result = await issueConnectCode(
      fetchImpl,
      "https://station.test",
      "/api/desktop/v2/connect-codes",
    );

    expect(result).toEqual({ code: "c1", origin: "https://station.test" });
    expect(fetchImpl).toHaveBeenCalledWith("/api/desktop/v2/connect-codes", {
      method: "POST",
      credentials: "include",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ origin: "https://station.test" }),
    });
  });

  it("maps API errors to YeschoyConnectError", async () => {
    const fetchImpl = vi.fn().mockResolvedValue({
      status: 401,
      json: async () => ({ success: false, code: "session_required" }),
    });

    await expect(
      issueConnectCode(fetchImpl, "https://station.test", "/api/desktop/v2/connect-codes"),
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
