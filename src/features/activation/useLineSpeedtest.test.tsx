import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { checkLineConnectivity } from "../../shared/api/connectivity";
import {
  CONNECTIVITY_LINES,
  type DecodedConnectivityProjection,
} from "../../diagnostics/contract";
import { useLineSpeedtest } from "./useLineSpeedtest";

vi.mock("../../shared/api/connectivity", () => ({
  checkLineConnectivity: vi.fn(),
}));
const check = vi.mocked(checkLineConnectivity);
function projection(latencies = [20, 10]): DecodedConnectivityProjection {
  return {
    complete: true,
    response: {
      requestId: "fixture-line",
      schemaVersion: 2,
      startedAtEpochMs: 1,
      completedAtEpochMs: 2,
      lines: CONNECTIVITY_LINES.map((line, index) => ({
        ...line,
        layers: [
          {
            layer: "dns",
            status: "passed",
            reasonCode: "dns_resolved",
            latencyMs: 1,
          },
          {
            layer: "tcp",
            status: "passed",
            reasonCode: "tcp443_reachable",
            latencyMs: 2,
          },
          {
            layer: "tls",
            status: "passed",
            reasonCode: "tls_handshake_verified",
            latencyMs: latencies[index],
          },
          {
            layer: "api_key",
            status: "skipped",
            reasonCode: "skipped_no_saved_session",
          },
        ],
      })),
    },
  };
}
beforeEach(() => {
  check.mockReset();
});
afterEach(cleanup);

describe("opt-in line speedtest ownership", () => {
  it("does not probe on mount or rerender; only an explicit action probes", async () => {
    check.mockResolvedValue(projection());
    const { result, rerender } = renderHook(useLineSpeedtest);
    rerender();
    expect(check).not.toHaveBeenCalled();
    expect(result.current.phase).toBe("idle");
    await act(async () => {
      await result.current.run();
    });
    expect(result.current.phase).toBe("done");
    expect(result.current.recommendedLine).toBe("global_accelerated");
    expect(result.current.lineLatencies).toEqual({
      mainland_optimized: 20,
      global_accelerated: 10,
    });
  });

  it("hides old recommendation while retrying and on failure", async () => {
    check
      .mockResolvedValueOnce(projection())
      .mockRejectedValueOnce(new Error("fixture_failure"));
    const { result } = renderHook(useLineSpeedtest);
    await act(async () => {
      await result.current.run();
    });
    expect(result.current.recommendedLine).not.toBeNull();
    await act(async () => {
      await result.current.run();
    });
    expect(result.current.phase).toBe("error");
    expect(result.current.recommendedLine).toBeNull();
    expect(result.current.lineLatencies).toEqual({});
  });

  it("ignores a slower superseded request and keeps the latest result", async () => {
    let finish!: (value: DecodedConnectivityProjection) => void;
    check
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finish = resolve;
          }),
      )
      .mockResolvedValueOnce(projection([5, 20]));
    const { result } = renderHook(useLineSpeedtest);
    let old!: Promise<void>;
    act(() => {
      old = result.current.run();
    });
    expect(result.current.phase).toBe("running");
    await act(async () => {
      await result.current.run();
    });
    await act(async () => {
      finish(projection());
      await old;
    });
    expect(result.current.recommendedLine).toBe("mainland_optimized");
    expect(result.current.lineLatencies.mainland_optimized).toBe(5);
  });

  it("lets an unmounted read settle without starting a replacement probe", async () => {
    let finish!: (value: DecodedConnectivityProjection) => void;
    check.mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const { result, unmount } = renderHook(useLineSpeedtest);
    let running!: Promise<void>;
    act(() => {
      running = result.current.run();
    });
    unmount();
    await act(async () => {
      finish(projection());
      await running;
    });
    expect(check).toHaveBeenCalledTimes(1);
  });
});
