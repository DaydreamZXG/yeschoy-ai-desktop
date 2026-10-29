import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import zh from "../i18n/locales/zh.json";
import { ResidueCard, decodeResidueInventory } from "./ResidueCard";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const invokeMock = vi.mocked(invoke);
const R = zh.yeschoyResidue;

const readyState = {
  id: "state",
  target: "state_dir",
  state: "ready",
  reasonCode: "owned_state",
  selectedByDefault: true,
  entries: ["connection-recovery", "device-id"],
};
const readyLogs = {
  id: "logs",
  target: "log_dir",
  state: "ready",
  reasonCode: "owned_logs",
  selectedByDefault: true,
  entries: ["yeschoy.log"],
};
const keptConfigs = {
  id: "app_configs",
  target: "tool_configs",
  state: "kept",
  reasonCode: "restore_via_app",
  selectedByDefault: false,
  entries: [],
};
const pendingKeychain = {
  id: "keychain",
  target: "keychain",
  state: "ready",
  reasonCode: "owned_keychain",
  selectedByDefault: true,
  entries: [],
};
const actionDisplaced = {
  id: "displaced_login",
  target: "keychain",
  state: "action",
  reasonCode: "login_taken_over",
  selectedByDefault: false,
  entries: [],
};

// 挂载会读一次盘点；StrictMode 下 effect 会重放，所以这里是「一直可用」的
// 桩，而不是只用一次的 mock。
function inventory(items: unknown[], clean = false) {
  return (args: unknown) => {
    const requestId = (args as { requestId: string }).requestId;
    return Promise.resolve({ requestId, clean, items });
  };
}

function stubInventory(items: unknown[], clean = false) {
  invokeMock.mockImplementation((_command, args) =>
    inventory(items, clean)(args),
  );
}

function stubInventoryCleared() {
  invokeMock.mockImplementation((command, args) => {
    const requestId = (args as { requestId: string }).requestId;
    if (command === "residue_clear_v1")
      return Promise.resolve({
        requestId,
        schemaVersion: 1,
        removed: [],
        remaining: { requestId, clean: true, items: [] },
      });
    return Promise.resolve({ requestId, clean: true, items: [] });
  });
}

function requestIdOf(callIndex: number) {
  return (invokeMock.mock.calls[callIndex][1] as { requestId: string })
    .requestId;
}

beforeEach(() => {
  invokeMock.mockReset();
});

describe("decodeResidueInventory", () => {
  it("accepts the native shape and rejects drift", () => {
    const ok = { requestId: "r", clean: false, items: [readyState] };
    expect(decodeResidueInventory(ok, "r")?.items).toHaveLength(1);
    expect(decodeResidueInventory(ok, "other")).toBeNull();
    expect(
      decodeResidueInventory(
        { ...ok, items: [{ ...readyState, extra: 1 }] },
        "r",
      ),
    ).toBeNull();
    expect(
      decodeResidueInventory(
        { ...ok, items: [{ ...readyState, state: "maybe" }] },
        "r",
      ),
    ).toBeNull();
    expect(
      decodeResidueInventory({ ...ok, items: [readyState, readyState] }, "r"),
    ).toBeNull();
  });
});

describe("residue card", () => {
  it("lists what is left, preselects only removable items, and previews paths", async () => {
    stubInventory([readyState, readyLogs, keptConfigs]);
    render(<ResidueCard />);
    // 每次 find/get 都只返回第一个匹配；StrictMode 的首次挂载已经失效，
    // 所以这里直接对整个文档断言，而不是某个列表容器。
    await waitFor(() =>
      expect(screen.getAllByRole("checkbox")).toHaveLength(2),
    );
    const boxes = screen.getAllByRole("checkbox") as HTMLInputElement[];
    expect(boxes.every((box) => box.checked)).toBe(true);
    // 有意保留的项没有勾选框，只有说明。
    expect(
      screen.getAllByText(R.reasons.restore_via_app.note).length,
    ).toBeGreaterThan(0);
    expect(screen.getAllByText(R.items.state.title).length).toBeGreaterThan(0);
    expect(screen.getAllByText(R.items.logs.title).length).toBeGreaterThan(0);
    expect(
      screen.getAllByText(R.items.app_configs.title).length,
    ).toBeGreaterThan(0);
    // 预览：路径只在展开后出现，且不含绝对路径。
    fireEvent.click(
      screen.getAllByText(R.previewTitle.replace("{{count}}", "2"))[0],
    );
    expect(screen.getByText("device-id")).toBeInTheDocument();
    expect(screen.getByText("connection-recovery")).toBeInTheDocument();
  });

  it("needs a second confirmation and sends only the ticked options", async () => {
    stubInventory([readyState, readyLogs, pendingKeychain]);
    render(<ResidueCard />);
    fireEvent.click(await screen.findByTestId("residue-clear"));
    // 第一次点击只展开确认，不调用删除。
    expect(
      invokeMock.mock.calls.filter(
        ([command]) => command === "residue_clear_v1",
      ),
    ).toHaveLength(0);

    // 取消勾选日志（第二项）。
    await waitFor(() =>
      expect(screen.getAllByRole("checkbox")).toHaveLength(3),
    );
    fireEvent.click(screen.getAllByRole("checkbox")[1]);

    stubInventoryCleared();
    fireEvent.click(screen.getByTestId("residue-confirm"));
    await screen.findByTestId("residue-cleared");

    const clearCall = invokeMock.mock.calls.find(
      ([command]) => command === "residue_clear_v1",
    );
    expect(clearCall?.[1]).toMatchObject({
      includeState: true,
      includeLogs: false,
      includeWebviewData: false,
      includeKeychain: true,
    });
  });

  it("never offers deletion while a restore is still pending", async () => {
    stubInventory([actionDisplaced]);
    render(<ResidueCard />);
    const warning = await screen.findByTestId("residue-restore-first");
    expect(warning).toHaveTextContent("1");
    expect(
      within(warning).getByText(/先在「我的应用」里恢复原设置/),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("residue-clear")).not.toBeInTheDocument();
  });

  it("says nothing is left when the inventory is clean", async () => {
    stubInventory([keptConfigs], true);
    render(<ResidueCard />);
    await screen.findByTestId("residue-clean");
    expect(screen.queryByTestId("residue-clear")).not.toBeInTheDocument();
  });

  it("keeps the list usable when the IPC fails", async () => {
    invokeMock.mockRejectedValueOnce(new Error("ipc down"));
    render(<ResidueCard />);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(R.errorTitle);
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(requestIdOf(0)).toMatch(/^residue-/);
  });

  it("reports a failed clear instead of pretending success", async () => {
    stubInventory([readyState]);
    render(<ResidueCard />);
    fireEvent.click(await screen.findByTestId("residue-clear"));
    invokeMock.mockRejectedValueOnce(new Error("locked by running app"));
    fireEvent.click(screen.getByTestId("residue-confirm"));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(R.partialTitle);
  });
});
