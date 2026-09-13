import { describe, expect, it, vi } from "vitest";
import type {
  ConnectivityLayerResult,
  ConnectivityLineResult,
} from "../diagnostics/contract";
import { lineLatencyMs, recommendedLineId } from "./ConfigurationPreviewView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}));

function layer(
  layer: ConnectivityLayerResult["layer"],
  status: ConnectivityLayerResult["status"],
  latencyMs?: number,
): ConnectivityLayerResult {
  return {
    layer,
    status,
    ...(latencyMs === undefined ? {} : { latencyMs }),
    reasonCode:
      layer === "dns"
        ? "dns_resolved"
        : layer === "tcp"
          ? "tcp443_reachable"
          : layer === "tls"
            ? "tls_handshake_verified"
            : "session_token_valid",
  };
}

function line(
  lineId: "mainland_optimized" | "global_accelerated",
  layers: ConnectivityLayerResult[],
): ConnectivityLineResult {
  return {
    lineId,
    displayName: lineId === "mainland_optimized" ? "大陆优化" : "全球加速",
    rootUrl:
      lineId === "mainland_optimized"
        ? "https://yeschoy.com"
        : "https://api.yeschoy.com",
    host: lineId === "mainland_optimized" ? "yeschoy.com" : "api.yeschoy.com",
    port: 443,
    layers,
  };
}

describe("lineLatencyMs 延迟回落优先级（#17）", () => {
  it("优先取 TLS 延迟", () => {
    const result = line("mainland_optimized", [
      layer("dns", "passed", 20),
      layer("tcp", "passed", 40),
      layer("tls", "passed", 80),
    ]);
    expect(lineLatencyMs(result)).toBe(80);
  });

  it("TLS 无延迟数据时回落 TCP", () => {
    const result = line("mainland_optimized", [
      layer("dns", "passed", 20),
      layer("tcp", "passed", 40),
      layer("tls", "passed"),
    ]);
    expect(lineLatencyMs(result)).toBe(40);
  });

  it("仅 DNS 有延迟时最后回落 DNS", () => {
    const result = line("mainland_optimized", [
      layer("dns", "passed", 20),
      layer("tcp", "passed"),
      layer("tls", "passed"),
    ]);
    expect(lineLatencyMs(result)).toBe(20);
  });

  it("无任何延迟数据返回 undefined", () => {
    const result = line("mainland_optimized", [
      layer("dns", "passed"),
      layer("tcp", "passed"),
      layer("tls", "passed"),
    ]);
    expect(lineLatencyMs(result)).toBeUndefined();
  });
});

describe("recommendedLineId 测速推荐（#17）", () => {
  it("仅网络可达且有延迟数据的线路参与，取最低延迟", () => {
    const lines = [
      line("mainland_optimized", [
        layer("dns", "passed", 10),
        layer("tcp", "passed", 50),
        layer("tls", "passed", 120),
      ]),
      line("global_accelerated", [
        layer("dns", "passed", 10),
        layer("tcp", "passed", 30),
        layer("tls", "passed", 60),
      ]),
    ];
    expect(recommendedLineId(lines)).toBe("global_accelerated");
  });

  it("不可达线路（tls 失败）即使延迟更低也不参与推荐", () => {
    const lines = [
      line("mainland_optimized", [
        layer("dns", "passed", 10),
        layer("tcp", "passed", 20),
        layer("tls", "failed", 25),
      ]),
      line("global_accelerated", [
        layer("dns", "passed", 10),
        layer("tcp", "passed", 30),
        layer("tls", "passed", 60),
      ]),
    ];
    expect(recommendedLineId(lines)).toBe("global_accelerated");
  });

  it("api_key 层失败不影响网络可达判断（未登录也可测速推荐）", () => {
    const lines = [
      line("mainland_optimized", [
        layer("dns", "passed", 10),
        layer("tcp", "passed", 30),
        layer("tls", "passed", 50),
        layer("api_key", "failed", 55),
      ]),
    ];
    expect(recommendedLineId(lines)).toBe("mainland_optimized");
  });

  it("无可达候选（全部无延迟）返回 null", () => {
    const lines = [
      line("mainland_optimized", [
        layer("dns", "passed"),
        layer("tcp", "passed"),
        layer("tls", "passed"),
      ]),
    ];
    expect(recommendedLineId(lines)).toBeNull();
  });

  it("空列表返回 null", () => {
    expect(recommendedLineId([])).toBeNull();
  });
});
