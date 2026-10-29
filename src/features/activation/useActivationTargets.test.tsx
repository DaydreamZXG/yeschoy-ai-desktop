import { StrictMode, type ReactNode } from "react";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { queryClient } from "../../lib/query/queryClient";
import {
  scanActivationTargets,
  type ActivationTargetScan,
} from "../../configuration/activation";
import { useActivationTargets } from "./useActivationTargets";

vi.mock("../../configuration/activation", async (original) => ({
  ...(await original<typeof import("../../configuration/activation")>()),
  scanActivationTargets: vi.fn(),
}));
const scan = vi.mocked(scanActivationTargets);
function fixture(
  ids = ["fixture-a", "fixture-b"],
  recommended = ids[0],
): ActivationTargetScan {
  return {
    requestId: "fixture-scan",
    schemaVersion: 1,
    platform: "macos",
    targets: [
      {
        toolId: "codex_desktop",
        displayName: "Codex",
        surface: "desktop",
        status: "available",
        manualLocation: "none",
        installations: ids.map((installationId) => ({
          installationId,
          label: installationId,
          version: "1.0",
          supported: true,
          recommended: installationId === recommended,
        })),
      },
    ],
  };
}
beforeEach(() => {
  queryClient.clear();
  scan.mockReset();
});
afterEach(() => {
  cleanup();
  queryClient.clear();
});

describe("activation target discovery state", () => {
  it("does not start a native scan while local activation is disabled", () => {
    const { result } = renderHook(() => useActivationTargets(false));
    expect(result.current.scanPhase).toBe("ready");
    expect(result.current.targetScan).toBeNull();
    expect(scan).not.toHaveBeenCalled();
  });

  it("preserves supported user selections across refresh and replaces vanished selections", async () => {
    scan
      .mockResolvedValueOnce(fixture())
      .mockResolvedValueOnce(fixture(["fixture-a", "fixture-b"], "fixture-a"))
      .mockResolvedValueOnce(fixture(["fixture-c"]));
    const { result } = renderHook(() => useActivationTargets(true));
    await waitFor(() => expect(result.current.scanPhase).toBe("ready"));
    act(() => result.current.selectInstallation("codex_desktop", "fixture-b"));
    await act(async () => {
      await result.current.refreshTargets();
    });
    expect(result.current.installations.codex_desktop).toBe("fixture-b");
    await act(async () => {
      await result.current.refreshTargets();
    });
    expect(result.current.installations.codex_desktop).toBe("fixture-c");
  });

  it("does not treat cached target data as current after a failed rescan", async () => {
    scan
      .mockResolvedValueOnce(fixture())
      .mockRejectedValueOnce(new Error("fixture_scan_failure"));
    const { result } = renderHook(() => useActivationTargets(true));
    await waitFor(() => expect(result.current.scanPhase).toBe("ready"));
    await act(async () => {
      await result.current.refreshTargets();
    });
    expect(result.current.scanPhase).toBe("error");
    expect(result.current.targetScan).toBeNull();
  });

  it("starts fresh evidence on overlapping refresh and prevents the older reply from overwriting it", async () => {
    let oldFinish!: (value: ActivationTargetScan) => void;
    scan
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            oldFinish = resolve;
          }),
      )
      .mockResolvedValueOnce(fixture(["fixture-new"]));
    const { result } = renderHook(() => useActivationTargets(true));
    await act(async () => {
      await result.current.refreshTargets();
    });
    expect(scan).toHaveBeenCalledTimes(2);
    expect(result.current.installations.codex_desktop).toBe("fixture-new");
    await act(async () => {
      oldFinish(fixture());
    });
    expect(result.current.installations.codex_desktop).toBe("fixture-new");
  });

  it("settles the active mount under StrictMode effect replay", async () => {
    scan.mockResolvedValue(fixture());
    const wrapper = ({ children }: { children: ReactNode }) => (
      <StrictMode>{children}</StrictMode>
    );
    const { result } = renderHook(() => useActivationTargets(true), {
      wrapper,
    });
    await waitFor(() => expect(result.current.scanPhase).toBe("ready"));
    expect(result.current.installations.codex_desktop).toBe("fixture-a");
  });
});
