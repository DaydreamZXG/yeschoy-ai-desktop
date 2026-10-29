import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import zh from "../i18n/locales/zh.json";
import {
  CursorConfigCard,
  decodeApplyResult,
  decodeCursorSnapshot,
  suggestedModel,
} from "./CursorConfigCard";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const invokeMock = vi.mocked(invoke);
const C = zh.yeschoyCursor;

const snapshot = {
  toolId: "cursor_agent",
  cliDetected: true,
  endpoint: "https://api.yeschoy.com/v1",
  customEndpointConfigured: false,
  headerNames: [],
  ide: {
    observed: true,
    customEndpointConfigured: false,
    existingProvider: "unknown",
  },
};

function stubSnapshot(overrides: Record<string, unknown> = {}) {
  const value = { ...snapshot, ...overrides };
  invokeMock.mockImplementation((command) =>
    Promise.resolve(command === "cursor_config_read_only" ? value : value),
  );
}

beforeEach(() => {
  invokeMock.mockReset();
});

describe("cursor decoders", () => {
  it("accepts the native snapshot and rejects unknown keys", () => {
    expect(decodeCursorSnapshot(snapshot)?.toolId).toBe("cursor_agent");
    expect(decodeCursorSnapshot({ ...snapshot, extra: 1 })).toBeNull();
    expect(
      decodeCursorSnapshot({ ...snapshot, toolId: "claude_code" }),
    ).toBeNull();
    expect(
      decodeCursorSnapshot({ ...snapshot, ide: { observed: true } }),
    ).toBeNull();
  });

  it("rejects unknown reason codes from the native side", () => {
    const ok = {
      toolId: "cursor_agent",
      status: "applied",
      reasonCode: "configured",
      modelId: "x",
    };
    expect(decodeApplyResult(ok)?.modelId).toBe("x");
    expect(decodeApplyResult({ ...ok, reasonCode: "whatever" })).toBeNull();
    expect(decodeApplyResult({ ...ok, modelId: 3 })).toBeNull();
  });

  it("suggests a real model from the catalog, never a made-up one", () => {
    const model = suggestedModel();
    expect(model).toBeTruthy();
    expect(model!.length).toBeGreaterThan(2);
  });
});

describe("cursor card", () => {
  it("shows the endpoint, the suggested model and the CLI state", async () => {
    stubSnapshot();
    render(<CursorConfigCard />);
    await screen.findByTestId("cursor-cli");
    expect(screen.getAllByText(snapshot.endpoint).length).toBeGreaterThan(0);
    expect(screen.getAllByText(C.facts.notConfigured).length).toBeGreaterThan(
      0,
    );
    expect(screen.getAllByTestId("cursor-model").length).toBeGreaterThan(0);
  });

  it("needs confirmation and sends only the model id", async () => {
    stubSnapshot();
    render(<CursorConfigCard />);
    const apply = await screen.findAllByTestId("cursor-apply");
    fireEvent.click(apply[apply.length - 1]);
    expect(
      invokeMock.mock.calls.filter(
        ([command]) => command === "cursor_apply_v1",
      ),
    ).toHaveLength(0);

    const confirm = screen.getAllByTestId("cursor-confirm");
    fireEvent.click(confirm[confirm.length - 1]);
    await waitFor(() =>
      expect(
        invokeMock.mock.calls.some(
          ([command]) => command === "cursor_apply_v1",
        ),
      ).toBe(true),
    );
    const call = invokeMock.mock.calls.find(([c]) => c === "cursor_apply_v1");
    expect(call?.[1]).toEqual({ request: { defaultModel: suggestedModel() } });
  });

  it("believes the native result: a failure is not reported as success", async () => {
    invokeMock.mockImplementation((command) => {
      if (command === "cursor_apply_v1")
        return Promise.resolve({
          toolId: "cursor_agent",
          status: "failed",
          reasonCode: "configuration_parse_failed",
          modelId: null,
        });
      return Promise.resolve(snapshot);
    });
    render(<CursorConfigCard />);
    fireEvent.click((await screen.findAllByTestId("cursor-apply")).pop()!);
    fireEvent.click(screen.getAllByTestId("cursor-confirm").pop()!);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(
      C.outcomes.configuration_parse_failed.title,
    );
    expect(alert).toHaveTextContent(C.outcomes.configuration_parse_failed.body);
  });

  it("offers revert only once an endpoint is configured", async () => {
    stubSnapshot({
      customEndpointConfigured: true,
      headerNames: ["X-Yeschoy-Desktop"],
    });
    render(<CursorConfigCard />);
    const revert = await screen.findAllByTestId("cursor-revert");
    expect(revert.length).toBeGreaterThan(0);
    fireEvent.click(revert[revert.length - 1]);
    await waitFor(() =>
      expect(
        invokeMock.mock.calls.some(
          ([command]) => command === "cursor_revert_v1",
        ),
      ).toBe(true),
    );
  });

  it("hides the CLI actions when the CLI is absent but still explains the desktop app", async () => {
    stubSnapshot({ cliDetected: false });
    render(<CursorConfigCard />);
    await screen.findAllByTestId("cursor-cli");
    expect(screen.getAllByText(C.cli.notDetected).length).toBeGreaterThan(0);
    expect(screen.queryByTestId("cursor-apply")).not.toBeInTheDocument();
    expect(screen.getAllByTestId("cursor-ide-steps").length).toBeGreaterThan(0);
    expect(screen.getAllByText(C.ide.warning).length).toBeGreaterThan(0);
  });

  it("renders a read error without pretending the app is connected", async () => {
    invokeMock.mockRejectedValueOnce(new Error("boom"));
    render(<CursorConfigCard />);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(C.readErrorTitle);
    expect(screen.queryByTestId("cursor-apply")).not.toBeInTheDocument();
  });
});
