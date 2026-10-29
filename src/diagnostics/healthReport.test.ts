import { describe, expect, it } from "vitest";
import {
  buildHealthReport,
  containsSensitiveText,
  decodeHealthSnapshot,
  healthFindings,
  type HealthSnapshot,
} from "./healthReport";
import type { ConnectivityResponse } from "./contract";

const snapshot: HealthSnapshot = {
  platform: "macos",
  arch: "aarch64",
  appVersion: "0.5.1",
  frontendMode: "bundled-custom-protocol",
  distribution: "community",
  logAvailable: true,
  logTail: [
    "1793171656.160 INFO yeschoy_desktop_lib::tool_discovery tool_discovery executable=claude path_directories=4 candidates=2",
  ],
  markers: [{ element: "desktop_shell", detail: "stage=start version=0.5.1" }],
};

const connectivity: ConnectivityResponse = {
  requestId: "line-1",
  schemaVersion: 2,
  startedAtEpochMs: 1,
  completedAtEpochMs: 2,
  lines: [
    {
      lineId: "mainland_optimized",
      displayName: "大陆优化",
      rootUrl: "https://yeschoy.com",
      host: "yeschoy.com",
      port: 443,
      layers: [
        {
          layer: "dns",
          status: "passed",
          reasonCode: "dns_resolved",
          latencyMs: 12,
        },
        {
          layer: "tcp",
          status: "passed",
          reasonCode: "tcp443_reachable",
          latencyMs: 30,
        },
        {
          layer: "tls",
          status: "failed",
          reasonCode: "tls_certificate_invalid",
          latencyMs: 41,
        },
        {
          layer: "api_key",
          status: "skipped",
          reasonCode: "skipped_upstream_failed",
        },
      ],
    },
    {
      lineId: "global_accelerated",
      displayName: "全球加速",
      rootUrl: "https://api.yeschoy.com",
      host: "api.yeschoy.com",
      port: 443,
      layers: [
        {
          layer: "dns",
          status: "passed",
          reasonCode: "dns_resolved",
          latencyMs: 8,
        },
        {
          layer: "tcp",
          status: "passed",
          reasonCode: "tcp443_reachable",
          latencyMs: 22,
        },
        {
          layer: "tls",
          status: "passed",
          reasonCode: "tls_handshake_verified",
          latencyMs: 30,
        },
        {
          layer: "api_key",
          status: "passed",
          reasonCode: "session_token_valid",
        },
      ],
    },
  ],
};

describe("decodeHealthSnapshot", () => {
  it("accepts the native shape and copies arrays", () => {
    const decoded = decodeHealthSnapshot(snapshot);
    expect(decoded).toEqual(snapshot);
    expect(decoded?.logTail).not.toBe(snapshot.logTail);
  });

  it("rejects extra, missing or mistyped fields", () => {
    expect(decodeHealthSnapshot({ ...snapshot, extra: true })).toBeNull();
    const { arch: _arch, ...missing } = snapshot;
    expect(decodeHealthSnapshot(missing)).toBeNull();
    expect(
      decodeHealthSnapshot({ ...snapshot, logAvailable: "yes" }),
    ).toBeNull();
    expect(decodeHealthSnapshot({ ...snapshot, platform: "beos" })).toBeNull();
    expect(decodeHealthSnapshot(null)).toBeNull();
  });

  it("rejects a log line that would leak a path or a key", () => {
    // The native scrubber is the first line of defence; this re-check means a
    // future native regression cannot reach the clipboard.
    for (const leak of [
      "opened /Users/someone/.codex/config.toml",
      "GET https://api.yeschoy.com/v1/models",
      "peer 192.168.1.10",
      "authorization sk-abcdefghijklmnop",
      "C:\\Users\\me\\AppData\\Roaming",
    ]) {
      expect(
        decodeHealthSnapshot({ ...snapshot, logTail: [leak] }),
        leak,
      ).toBeNull();
    }
  });

  it("detects sensitive shapes independently", () => {
    expect(containsSensitiveText("candidates=2")).toBe(false);
    expect(containsSensitiveText("eyJhbGciOiJIUzI1NiJ9.payload.sig")).toBe(
      true,
    );
  });
});

describe("healthFindings", () => {
  it("reports one finding per failed line, pointing at the first failing layer", () => {
    expect(healthFindings(connectivity)).toEqual([
      {
        lineId: "mainland_optimized",
        layer: "tls",
        reasonCode: "tls_certificate_invalid",
        outcome: "tls_failed",
      },
    ]);
  });

  it("reports nothing when every line passed", () => {
    expect(
      healthFindings({ ...connectivity, lines: [connectivity.lines[1]] }),
    ).toEqual([]);
  });
});

describe("buildHealthReport", () => {
  it("orders sections deterministically and carries the reason codes", () => {
    const report = buildHealthReport({
      generatedAt: new Date("2026-10-29T02:00:00.000Z"),
      snapshot,
      connectivity,
      userNote: "点了接入没有反应",
    });
    const text = report.split("\n");
    expect(text[0]).toBe("Yeschoy health report (schema v1)");
    expect(text).toContain("generated-at: 2026-10-29T02:00:00.000Z");
    expect(text).toContain(
      "env: platform=macos arch=aarch64 app=0.5.1 frontend=bundled-custom-protocol distribution=community",
    );
    expect(text).toContain("mainland_optimized: tls_failed");
    expect(text).toContain("  tls failed 41ms tls_certificate_invalid");
    expect(text).toContain("  api_key skipped skipped_upstream_failed");
    expect(report.indexOf("[connectivity]")).toBeLessThan(
      report.indexOf("[stage-markers]"),
    );
    expect(report.indexOf("[stage-markers]")).toBeLessThan(
      report.indexOf("[log-tail]"),
    );
    expect(report.endsWith("[what-happened]\n点了接入没有反应")).toBe(true);
    expect(containsSensitiveText(report)).toBe(false);
  });

  it("degrades to explicit markers instead of dropping sections", () => {
    const report = buildHealthReport({
      generatedAt: new Date("2026-10-29T02:00:00.000Z"),
      snapshot: null,
      connectivity: null,
      userNote: "   ",
    });
    expect(report).toContain("env: unavailable");
    expect(report).toContain("[connectivity]\nnot-checked");
    expect(report).toContain("[log-tail]\nunavailable");
    expect(report.endsWith("[what-happened]\nnone")).toBe(true);
  });

  it("distinguishes a disabled log from an empty one", () => {
    const base = {
      generatedAt: new Date(0),
      connectivity: null,
      userNote: "",
    };
    expect(
      buildHealthReport({
        ...base,
        snapshot: {
          ...snapshot,
          logAvailable: false,
          logTail: [],
          markers: [],
        },
      }),
    ).toContain("[log-tail]\nlog-not-available");
    expect(
      buildHealthReport({ ...base, snapshot: { ...snapshot, logTail: [] } }),
    ).toContain("[log-tail]\nempty");
    expect(
      buildHealthReport({
        ...base,
        snapshot: { ...snapshot, markers: [] },
      }),
    ).toContain("[stage-markers]\nnone");
  });

  it("caps a pasted user note", () => {
    const report = buildHealthReport({
      generatedAt: new Date(0),
      snapshot,
      connectivity: null,
      userNote: "x".repeat(5000),
    });
    expect(report.split("\n").at(-1)).toHaveLength(2000);
  });
});
