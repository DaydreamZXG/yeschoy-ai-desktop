import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import zh from "../i18n/locales/zh.json";
import type { AccountProjection } from "../account/session";
import type { AccountSessionController } from "../account/useAccountSession";
import { AccountView } from "./AccountView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeAll(async () => {
  await i18n.init({
    lng: "zh",
    resources: { zh: { translation: zh } },
    interpolation: { escapeValue: false },
  });
});

afterEach(cleanup);

function session(
  status: AccountProjection["status"],
): AccountSessionController {
  const projection: AccountProjection = {
    requestId: "fixture",
    schemaVersion: 3,
    status,
    userCode: "",
    pollAfterSeconds: 0,
    expiresAtEpochMs: 0,
    observedAtEpochMs: 1000,
    account: {
      available: false,
      displayName: "",
      username: "",
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
    reasonCode: status,
  };
  return {
    projection,
    loading: false,
    refresh: vi.fn(),
    beginAuthorization: vi.fn(),
    openAuthorization: vi.fn(),
    cancelAuthorization: vi.fn(),
    logout: vi.fn(),
    openWallet: vi.fn(),
  };
}

describe("AccountView recovery copy", () => {
  it("suggests switching relay lines when inspect is unreachable", () => {
    render(
      <AccountView
        lineId="mainland_optimized"
        onLineChange={vi.fn()}
        onOpenAppsHome={vi.fn()}
        session={session("backend_unavailable")}
      />,
    );
    expect(screen.getByTestId("account-relay-line-recovery")).toHaveTextContent(
      "全球加速",
    );
    expect(
      screen.getByRole("button", {
        name: zh.workbench.accountEnterStationOrigin,
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(zh.workbench.accountEnterStationOriginHint),
    ).toBeInTheDocument();
    expect(screen.queryByText(/换站点/)).toBeNull();
  });
});
