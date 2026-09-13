import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import i18n from "i18next";
import App from "../App";
import zh from "../i18n/locales/zh.json";
import tw from "../i18n/locales/zh-TW.json";
import en from "../i18n/locales/en.json";
import ja from "../i18n/locales/ja.json";
import {
  TOOL_CATALOG,
  decodeScan,
  type ScanResponse,
} from "../tool-discovery/contract";
import { createAppearanceSync } from "./nativeAppearance";
import { useAppearance, APPEARANCE_KEY } from "./appearance";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
import { installationInspectionFixture } from "../installation/test-fixtures";
const native = vi.mocked(invoke);
function mockNativeByCommand(
  handler: (command: string, args: unknown) => unknown,
) {
  native.mockImplementation(async (command, args) => {
    if (command === "read_desktop_exit_state") {
      return { closeRequested: false, shutdown: null };
    }
    if (command === "manage_app_installation_v2")
      return installationInspectionFixture(args);
    return handler(command, args);
  });
}
function fixture(requestId = "scan-1"): ScanResponse {
  return {
    requestId,
    platform: "macos",
    startedAtEpochMs: 1,
    completedAtEpochMs: 2,
    tools: TOOL_CATALOG.map((tool) => ({
      toolId: tool.id,
      displayName: tool.displayName,
      status: "not_found",
      version: "",
      candidateCount: 0,
      bundledCount: 0,
      selection: "not_found",
      locationHint: "none",
      compatibility: "not_applicable",
      reasonCode: "tool_not_found",
    })),
  };
}
function found(scan: ScanResponse) {
  Object.assign(scan.tools[1], {
    status: "detected_unverified",
    version: "0.146.0",
    candidateCount: 1,
    bundledCount: 1,
    selection: "single_installation",
    locationHint: "path",
    compatibility: "unverified_read_only",
    reasonCode: "exact_version_not_allowlisted",
  });
  return scan;
}
const tick = async () => {
  await act(async () => {
    await Promise.resolve();
  });
};
let systemDark = false;
const listeners = new Set<() => void>();
beforeEach(async () => {
  for (const [locale, resource] of Object.entries({ zh, "zh-TW": tw, en, ja }))
    i18n.addResourceBundle(locale, "translation", resource, true, true);
  await i18n.changeLanguage("zh");
  localStorage.removeItem(APPEARANCE_KEY);
  native.mockReset();
  mockNativeByCommand(() => {
    throw Error("unavailable");
  });
  systemDark = false;
  listeners.clear();
  vi.stubGlobal("scrollTo", vi.fn());
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({
      get matches() {
        return systemDark;
      },
      addEventListener: (_: string, fn: () => void) => listeners.add(fn),
      removeEventListener: (_: string, fn: () => void) => listeners.delete(fn),
    })),
  );
});
afterEach(() => {
  for (const [command, args] of native.mock.calls) {
    if (command === "read_desktop_exit_state") expect(args).toBeUndefined();
    if (command === "manage_app_installation_v2")
      expect(() => installationInspectionFixture(args)).not.toThrow();
  }
  cleanup();
  vi.unstubAllGlobals();
});

describe("v2 projection boundary", () => {
  it("accepts standalone plus bundled and deterministic PATH selection", () => {
    const scan = found(fixture());
    expect(decodeScan(scan, "scan-1")).toEqual(scan);
    Object.assign(scan.tools[1], {
      candidateCount: 3,
      selection: "path_precedence",
    });
    expect(decodeScan(scan, "scan-1")).toEqual(scan);
  });
  it.each([
    (s: ScanResponse) => {
      s.requestId = "old";
    },
    (s: ScanResponse) => {
      s.tools.reverse();
    },
    (s: ScanResponse) => {
      s.tools.pop();
    },
    (s: ScanResponse) => {
      s.tools[1] = s.tools[0];
    },
    (s: ScanResponse) => {
      s.tools[1].version = "/Users/private/key";
    },
    (s: ScanResponse) => {
      s.completedAtEpochMs = Number.NaN;
    },
    (s: ScanResponse) => {
      s.completedAtEpochMs = 0;
    },
    (s: ScanResponse) => {
      s.tools[1].candidateCount = 32;
    },
    (s: ScanResponse) => {
      s.tools[1].candidateCount = -1;
    },
    (s: ScanResponse) => {
      s.tools[1].selection = "unresolved";
    },
    (s: ScanResponse) => {
      s.tools[1].compatibility = "not_applicable";
    },
    (s: ScanResponse) => {
      s.tools[1].bundledCount = 33;
    },
    (s: ScanResponse) => {
      s.platform = "windows";
    },
    (s: ScanResponse) => {
      Object.assign(s.tools[1], { path: "/Users/private" });
    },
    (s: ScanResponse) => {
      Object.assign(s.tools[1], {
        candidateCount: 2,
        selection: "path_precedence",
        locationHint: "common_location",
      });
    },
  ])("rejects invalid or contradictory evidence %#", (mutate) => {
    const scan = found(fixture());
    mutate(scan);
    expect(decodeScan(scan, "scan-1")).toBeNull();
  });
  it.each([null, {}, [], "ok", { tools: null }])(
    "rejects malformed native data %#",
    (value) => {
      expect(decodeScan(value, "scan-1")).toBeNull();
    },
  );
  it("accepts only truthful failure, bundled-only and unresolved states", () => {
    for (const [status, reasonCode] of [
      ["probe_failed", "version_command_failed"],
      ["probe_timed_out", "version_command_timed_out"],
    ]) {
      const s = found(fixture());
      Object.assign(s.tools[1], { status, reasonCode, version: "" });
      expect(decodeScan(s, "scan-1")).not.toBeNull();
    }
    const bundled = fixture();
    Object.assign(bundled.tools[1], {
      bundledCount: 1,
      selection: "bundled_only",
    });
    expect(decodeScan(bundled, "scan-1")).not.toBeNull();
    const conflict = found(fixture());
    Object.assign(conflict.tools[1], {
      candidateCount: 2,
      selection: "unresolved",
      version: "",
      locationHint: "multiple",
      status: "multiple_installations",
      reasonCode: "multiple_executables_found",
    });
    expect(decodeScan(conflict, "scan-1")).not.toBeNull();
  });
});

async function openTools() {
  render(<App />);
  await tick();
  fireEvent.click(screen.getByRole("button", { name: "已安装的工具" }));
}
function scanWith(make: (id: string) => unknown) {
  mockNativeByCommand(async (command, args) => {
    if (command !== "scan_tools_read_only_v2") throw Error("unavailable");
    return make((args as { request: { requestId: string } }).request.requestId);
  });
}
describe("beginner scan recovery", () => {
  it("identifies the standalone CLI without a selection chore or false connection claim", async () => {
    scanWith((id) => found(fixture(id)));
    await openTools();
    fireEvent.click(screen.getByTestId("scan-tools-action"));
    await tick();
    const card = screen
      .getByRole("heading", { name: "Codex（命令行）" })
      .closest("article")!;
    expect(within(card).getByText("已自动识别独立安装")).toBeVisible();
    expect(within(card).getByText("0.146.0")).toBeVisible();
    expect(within(card).queryByText("发现多处安装")).toBeNull();
    expect(card.textContent).toContain("无需选择或删除");
    expect(card.textContent).toContain("尚未开放自动配置");
    expect(card.textContent).not.toMatch(/已接通|已连接/);
    expect(native.mock.calls.map(([cmd]) => cmd)).toEqual([
      "scan_activation_targets_v1",
      "manage_app_installation_v2",
      "manage_tool_connections_v1",
      "account_inspect_v2",
      "read_desktop_exit_state",
      "scan_tools_read_only_v2",
    ]);
  });
  it("shows actionable unresolved state and recovers on retry", async () => {
    scanWith((id) => {
      const s = found(fixture(id));
      Object.assign(s.tools[1], {
        candidateCount: 2,
        selection: "unresolved",
        version: "",
        locationHint: "multiple",
        status: "multiple_installations",
        reasonCode: "multiple_executables_found",
      });
      return s;
    });
    await openTools();
    fireEvent.click(screen.getByTestId("scan-tools-action"));
    await tick();
    const card = screen
      .getByRole("heading", { name: "Codex（命令行）" })
      .closest("article")!;
    expect(
      within(card).getByRole("button", { name: "查看桌面应用" }),
    ).toBeEnabled();
    scanWith((id) => found(fixture(id)));
    fireEvent.click(within(card).getByRole("button", { name: "重新检查" }));
    await tick();
    expect(screen.getByText("0.146.0")).toBeVisible();
    expect(screen.queryByText("发现多处安装")).toBeNull();
  });
  it("routes bundled-only users back to desktop apps without removing anything", async () => {
    scanWith((id) => {
      const s = fixture(id);
      Object.assign(s.tools[1], { bundledCount: 1, selection: "bundled_only" });
      return s;
    });
    await openTools();
    fireEvent.click(screen.getByTestId("scan-tools-action"));
    await tick();
    expect(screen.getByText("桌面应用组件")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "查看桌面应用" }));
    await tick();
    expect(document.querySelector("main")?.getAttribute("data-view")).toBe(
      "setup",
    );
    expect(
      native.mock.calls.every(([cmd]) =>
        [
          "scan_desktop_apps_read_only",
          "manage_app_installation_v2",
          "manage_tool_connections_v1",
          "account_inspect_v2",
          "scan_tools_read_only_v2",
          "scan_activation_targets_v1",
          "read_desktop_exit_state",
        ].includes(cmd),
      ),
    ).toBe(true);
    expect(
      native.mock.calls.filter(([cmd]) => cmd === "read_desktop_exit_state"),
    ).toHaveLength(1);
  });
  it("rejects stale and malformed responses, and offers retry after failure", async () => {
    scanWith(() => found(fixture("old-request")));
    await openTools();
    fireEvent.click(screen.getByTestId("scan-tools-action"));
    await tick();
    expect(screen.getByRole("alert")).toHaveTextContent("这次检查没有完成");
    expect(screen.queryByText("0.146.0")).toBeNull();
    scanWith(() => ({ tools: null }));
    fireEvent.click(screen.getByTestId("scan-tools-action"));
    await tick();
    expect(screen.getByRole("alert")).toBeVisible();
    scanWith((id) => found(fixture(id)));
    fireEvent.click(screen.getByTestId("scan-tools-action"));
    await tick();
    expect(screen.queryByRole("alert")).toBeNull();
  });
  it("keeps all four localized recovery actions and explanations nonempty", () => {
    for (const locale of [zh, tw, en, ja]) {
      for (const key of [
        "codexCli",
        "bundledStatus",
        "componentDetails",
        "componentsIgnored",
        "desktopAction",
      ] as const)
        expect(locale.yeschoyDiscovery[key].length).toBeGreaterThan(2);
      expect(Object.keys(locale.yeschoyDiscovery.selection).sort()).toEqual([
        "bundled_only",
        "path_precedence",
        "single_installation",
      ]);
    }
  });
});

describe("native appearance ordering", () => {
  it("serializes assignments and coalesces rapid choices to latest", async () => {
    const completions: (() => void)[] = [];
    const apply = vi.fn(
      () => new Promise<void>((resolve) => completions.push(resolve)),
    );
    const sync = createAppearanceSync(apply);
    sync("dark");
    sync("light");
    sync("dark");
    sync("light");
    expect(apply.mock.calls).toEqual([["dark"]]);
    completions.shift()!();
    await tick();
    expect(apply.mock.calls).toEqual([["dark"], ["light"]]);
    completions.shift()!();
    await tick();
    sync("dark");
    expect(apply.mock.calls).toHaveLength(3);
    completions.shift()!();
    await tick();
  });
  it("a rejected native assignment does not prevent the next change", async () => {
    const apply = vi
      .fn()
      .mockRejectedValueOnce(Error("unavailable"))
      .mockResolvedValue(undefined);
    const sync = createAppearanceSync(apply);
    sync("dark");
    sync("light");
    await tick();
    expect(apply.mock.calls).toEqual([["dark"], ["light"]]);
  });
  it("syncs fixed palette and follows real system changes after leaving manual mode", async () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    mockNativeByCommand((command) => {
      if (command === "set_window_appearance") return {};
      throw Error(`Unexpected native command: ${command}`);
    });
    function Harness() {
      const { changeAppearance } = useAppearance();
      return (
        <>
          {(["light", "dark", "system"] as const).map((value) => (
            <button key={value} onClick={() => changeAppearance(value)}>
              {value}
            </button>
          ))}
        </>
      );
    }
    const mounted = render(<Harness />);
    await tick();
    fireEvent.click(screen.getByText("dark"));
    await tick();
    expect(document.documentElement.dataset.theme).toBe("dark");
    fireEvent.click(screen.getByText("system"));
    await tick();
    expect(document.documentElement.dataset.theme).toBe("light");
    systemDark = true;
    act(() => listeners.forEach((fn) => fn()));
    await tick();
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(native.mock.calls.map(([cmd, args]) => [cmd, args])).toEqual(
      ["light", "dark", "light", "dark"].map((theme) => [
        "set_window_appearance",
        { request: { theme } },
      ]),
    );
    mounted.unmount();
    expect(listeners.size).toBe(0);
  });
});
