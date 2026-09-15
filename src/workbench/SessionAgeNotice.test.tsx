import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import zh from "../i18n/locales/zh.json";
import type { AccountProjection } from "../account/session";
import type { AccountSessionController } from "../account/useAccountSession";
import { DAY_MS } from "../account/sessionAge";
import { AccountView } from "./AccountView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}));

const STORAGE_KEY = "yeschoy.account.sessionAge.v1";

beforeAll(async () => {
  await i18n.init({
    lng: "zh",
    resources: { zh: { translation: zh } },
    interpolation: { escapeValue: false },
  });
});
beforeEach(() => {
  window.localStorage.clear();
});
afterEach(cleanup);

function projection(): AccountProjection {
  return {
    requestId: "ui-age",
    schemaVersion: 3,
    status: "signed_in",
    userCode: "",
    pollAfterSeconds: 0,
    expiresAtEpochMs: 0,
    observedAtEpochMs: 3000,
    account: {
      available: true,
      displayName: "普通用户",
      username: "member",
      balanceQuota: "13295000",
      usedQuota: "165330000",
      requestCount: "3274",
      quotaPerUnit: "500000",
    },
    usage: {
      available: true,
      consumedQuota: "1",
      requestRate: "3",
      tokenCount: "999999",
    },
    models: [],
    comparisonFx: "1",
    reasonCode: "none",
  };
}

function controller(): AccountSessionController {
  return {
    projection: projection(),
    loading: false,
    lastError: null,
    refresh: vi.fn(),
    beginAuthorization: vi.fn(),
    openAuthorization: vi.fn(),
    cancelAuthorization: vi.fn(),
    logout: vi.fn(),
    openWallet: vi.fn(),
  };
}

function seedAge(authDaysAgo: number, lastUseDaysAgo: number): void {
  const now = Date.now();
  window.localStorage.setItem(
    STORAGE_KEY,
    JSON.stringify({
      authorizedAtEpochMs: now - authDaysAgo * DAY_MS,
      lastUsedAtEpochMs: now - lastUseDaysAgo * DAY_MS,
    }),
  );
}

// 恰好踩在阈值上的用例（25/80 天）留 1 小时余量，
// 避免种子与渲染之间数毫秒的流逝把 ">N 天" 提前触发。
const HOUR_IN_DAYS = 1 / 24;

function view(session = controller()) {
  return render(
    <AccountView
      lineId="mainland_optimized"
      onLineChange={vi.fn()}
      session={session}
    />,
  );
}

describe("会话时效提示（#23：25/80 天两级，仅提示不阻断）", () => {
  it("距上次使用 26 天：显示过期提示，账户内容仍可见，无重新授权按钮", () => {
    seedAge(26, 26);
    view();
    const notice = screen.getByTestId("session-age-notice");
    expect(notice).toHaveAttribute("role", "status");
    expect(
      screen.getByText(/距上次使用已超过 25 天，会话可能即将过期/),
    ).toBeInTheDocument();
    // 不阻断主流程：余额、身份卡、充值入口照常渲染。
    expect(screen.getByText("当前账户")).toBeInTheDocument();
    expect(screen.getByText("去充值")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "重新授权" }),
    ).not.toBeInTheDocument();
  });

  it("距上次使用 25 天：未过阈值，无提示", () => {
    seedAge(25 - HOUR_IN_DAYS, 25 - HOUR_IN_DAYS);
    view();
    expect(screen.queryByTestId("session-age-notice")).not.toBeInTheDocument();
  });

  it("距授权 81 天：显示重新授权引导，点击发起重新授权且不退出当前会话", () => {
    seedAge(81, 1);
    const session = controller();
    view(session);
    expect(
      screen.getByText(/本次授权已超过 80 天，接近 90 天强制重新授权的期限/),
    ).toBeInTheDocument();
    const action = screen.getByRole("button", { name: "重新授权" });
    fireEvent.click(action);
    expect(session.beginAuthorization).toHaveBeenCalledTimes(1);
    expect(session.logout).not.toHaveBeenCalled();
    // 不阻断：身份卡与刷新入口仍在。
    expect(screen.getByText("当前账户")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "刷新数据" })).toBeInTheDocument();
  });

  it("距授权 80 天：仍为过期提示级，不出现重新授权按钮", () => {
    seedAge(80 - HOUR_IN_DAYS, 1);
    view();
    expect(screen.getByTestId("session-age-notice")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "重新授权" }),
    ).not.toBeInTheDocument();
  });

  it("无本地记录或记录新鲜：无提示", () => {
    view();
    expect(screen.queryByTestId("session-age-notice")).not.toBeInTheDocument();
    seedAge(1, 1);
    view();
    expect(screen.queryByTestId("session-age-notice")).not.toBeInTheDocument();
  });
});
