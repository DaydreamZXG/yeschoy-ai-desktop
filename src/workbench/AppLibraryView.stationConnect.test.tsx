import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import zh from "../i18n/locales/zh.json";
import {
  AppLibraryView,
  showStationConnectWelcomeStrip,
} from "./AppLibraryView";
import { ConnectionProvider } from "../configuration/connections";
import { connectionsFixture } from "../configuration/connection-test-fixtures";
import type { AccountSessionController } from "../account/useAccountSession";
import type { AccountProjection, AccountStatus } from "../account/session";

function projection(status: AccountStatus): AccountProjection {
  const signedIn = status === "signed_in";
  return {
    requestId: "fixture",
    schemaVersion: 3,
    status,
    userCode: "",
    pollAfterSeconds: 0,
    expiresAtEpochMs: 0,
    observedAtEpochMs: 1000,
    account: signedIn
      ? {
          available: true,
          displayName: "普通用户",
          username: "user535",
          balanceQuota: "10",
          usedQuota: "1",
          requestCount: "1",
          quotaPerUnit: "500000",
        }
      : {
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
    models: signedIn
      ? [
          {
            id: "model-a",
            description: "",
            billingMode: "ratio",
            supportedEndpointTypes: ["openai"],
            pricingAvailable: false,
            officialInputCnyPerMillion: "",
            officialOutputCnyPerMillion: "",
            actualInputCnyPerMillion: "",
            actualOutputCnyPerMillion: "",
            billing: null,
          },
        ]
      : [],
    comparisonFx: "",
    reasonCode: signedIn ? "none" : status,
  };
}

function sessionFor(
  status: AccountStatus,
  extra: Partial<AccountSessionController> = {},
): AccountSessionController {
  const base = projection(status);
  return {
    projection: base,
    loading: false,
    refresh: vi.fn(async () => base),
    beginAuthorization: vi.fn(async () => null),
    openAuthorization: vi.fn(async () => null),
    cancelAuthorization: vi.fn(async () => null),
    logout: vi.fn(async () => null),
    openWallet: vi.fn(async () => true),
    ...extra,
  };
}

const connections = {
  ...connectionsFixture("fixture"),
  loading: false,
  error: false,
  refreshing: false,
  restoring: null,
  opening: null,
  refresh: vi.fn(async () => {}),
  restore: vi.fn(async () => connectionsFixture("restore")),
  open: vi.fn(async () => ({
    status: "opened" as const,
    reasonCode: "opened",
  })),
};

beforeEach(async () => {
  i18n.addResourceBundle("zh", "translation", zh, true, true);
  await i18n.changeLanguage("zh");
  vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
  const { invoke } = await import("@tauri-apps/api/core");
  vi.mocked(invoke).mockResolvedValue({
    requestId: "scan",
    schemaVersion: 1,
    platform: "macos",
    targets: [],
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("showStationConnectWelcomeStrip", () => {
  it.each([
    "signed_out",
    "session_expired",
    "cancelled",
    "denied",
    "expired",
    "backend_unavailable",
    "network_error",
    "incompatible_server",
    "invalid_response",
    "secure_storage_unavailable",
  ] as AccountStatus[])("shows the strip for %s", (status) => {
    expect(showStationConnectWelcomeStrip(status)).toBe(true);
  });

  it.each([undefined, null])("shows the strip when status is %s", (status) => {
    expect(showStationConnectWelcomeStrip(status)).toBe(true);
  });

  it.each(["signed_in", "authorization_pending"] as AccountStatus[])(
    "hides the strip for %s",
    (status) => {
      expect(showStationConnectWelcomeStrip(status)).toBe(false);
    },
  );
});

describe("AppLibraryView station connect welcome strip", () => {
  function renderHome(account: AccountSessionController) {
    return render(
      <ConnectionProvider value={connections}>
        <AppLibraryView
          accountSession={account}
          onOpenAccount={vi.fn()}
          onOpenSetup={vi.fn()}
          onOpenCommunity={vi.fn()}
          onStationLineChange={vi.fn()}
          accountLineId="mainland_optimized"
        />
      </ConnectionProvider>,
    );
  }

  it("renders StationConnectOriginForm when inspect reports backend_unavailable", async () => {
    renderHome(
      sessionFor("backend_unavailable", { lastError: "server_not_ready" }),
    );
    expect(screen.getByRole("textbox")).toBeInTheDocument();
    expect(screen.getByText(zh.workbench.librarySignInTitle)).toBeInTheDocument();
    expect(screen.getByTestId("library-relay-line-recovery")).toHaveTextContent(
      "全球加速",
    );
  });

  it("does not render the origin form when signed in", async () => {
    renderHome(sessionFor("signed_in"));
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(
      screen.getByRole("region", { name: zh.yeschoyDaily.accountOverview }),
    ).toBeInTheDocument();
  });

  it("does not render the origin form while authorization is pending", async () => {
    renderHome(sessionFor("authorization_pending"));
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(screen.queryByText(zh.workbench.librarySignInTitle)).toBeNull();
  });

  it("keeps the origin form interactive with idle signed-out session (no probe)", async () => {
    renderHome(
      sessionFor("signed_out", {
        loading: false,
      }),
    );
    expect(
      screen.getByRole("button", { name: zh.stationConnectPage.continue }),
    ).toBeEnabled();
    expect(
      screen.queryByText(zh.stationConnectPage.accountChecking),
    ).not.toBeInTheDocument();
  });
});
