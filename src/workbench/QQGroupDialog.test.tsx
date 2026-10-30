import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import zh from "../i18n/locales/zh.json";
import type { AccountProjection } from "../account/session";
import {
  QQGroupDialog,
  QQ_GROUP_ID,
  CommunityGroupDialog,
} from "./QQGroupDialog";

function signedInProjection(): AccountProjection {
  return {
    requestId: "req",
    schemaVersion: 3,
    status: "signed_in",
    userCode: "",
    pollAfterSeconds: 0,
    expiresAtEpochMs: 0,
    observedAtEpochMs: 0,
    account: {
      available: true,
      displayName: "u",
      username: "u",
      balanceQuota: "",
      usedQuota: "",
      requestCount: "",
      quotaPerUnit: "",
    },
    usage: {
      available: false,
      consumedQuota: "",
      requestRate: "",
      tokenCount: "",
    },
    models: [],
    comparisonFx: "",
    reasonCode: "signed_in",
  };
}

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
// sonner toast 在 jsdom 下直接 mock，避免等待真实 DOM
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

const copy = {
  title: "野菜API QQ 交流群",
  body: "扫码或复制群号加群",
  groupIdLabel: "群号",
  scanHint: "用 QQ 扫码",
  copyGroupId: "复制群号",
  groupIdCopied: "已复制",
  close: "关闭",
};

beforeAll(async () => {
  await i18n.init({
    lng: "zh",
    resources: { zh: { translation: zh } },
  });
});

afterEach(cleanup);

describe("CommunityGroupDialog", () => {
  it("renders when signed in to YesChoy on OSS builds", () => {
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "oss");
    render(
      <CommunityGroupDialog
        open
        onOpenChange={() => {}}
        lineId="mainland_optimized"
        accountProjection={signedInProjection()}
      />,
    );
    expect(screen.getByRole("dialog")).toBeTruthy();
    vi.unstubAllEnvs();
  });

  it("does not render for third-party station sessions", () => {
    const { container } = render(
      <CommunityGroupDialog
        open
        onOpenChange={() => {}}
        lineId="station_connect:https://partner.example.com"
        accountProjection={signedInProjection()}
      />,
    );
    expect(container.querySelector(".qq-group-dialog")).toBeNull();
  });
});

describe("QQGroupDialog", () => {
  it("不渲染任何东西当 closed", () => {
    const { container } = render(
      <QQGroupDialog open={false} onOpenChange={() => {}} copy={copy} />,
    );
    // Dialog closed → 无 dialog role
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(container.querySelector(".qq-group-dialog")).toBeNull();
  });

  it("打开时显示标题、群号、复制与关闭按钮", () => {
    render(<QQGroupDialog open onOpenChange={() => {}} copy={copy} />);
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(screen.getByText(copy.title)).toBeTruthy();
    // 群号出现在显眼位置（群名由官方卡片图自带，不再重复渲染）
    expect(screen.getByText(QQ_GROUP_ID)).toBeTruthy();
    expect(screen.getByRole("button", { name: copy.copyGroupId })).toBeTruthy();
    expect(
      screen.getAllByRole("button", { name: copy.close }).length,
    ).toBeGreaterThanOrEqual(1);
  });

  it("群号固定为 1065665694（不允许随意改动）", () => {
    expect(QQ_GROUP_ID).toBe("1065665694");
  });
});
