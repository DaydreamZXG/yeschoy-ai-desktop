import { describe, expect, it } from "vitest";
import type { ToolConnection } from "../configuration/connections";
import { originOfLine, reportOf, type CheckInput } from "./checkup";

const app = (
  toolId: ToolConnection["toolId"],
  state: ToolConnection["state"],
): ToolConnection => ({
  toolId,
  state,
  modelId: "m",
  lineId: "",
  billingGroup: "",
  updatedAtEpochMs: 0,
  restoreMode: "original",
  requiresBackground: false,
  reasonCode: "",
});
const good: CheckInput = {
  online: true,
  probe: { ok: true, probe: {} as never },
  status: "signed_in",
  connections: [app("claude_code", "connected")],
  connectionsFailed: false,
};
const check = (extra: Partial<CheckInput>) => reportOf({ ...good, ...extra });

describe("reportOf", () => {
  it("三样都好就说都好，不给按钮", () => {
    expect(check({})).toMatchObject({
      station: "ok",
      login: "ok",
      apps: "ok",
      live: 1,
      verdict: "fine",
      action: null,
    });
  });

  it("电脑没联网和站点连不上分开说", () => {
    const down = { ok: false as const, reason: "unreachable" };
    expect(check({ probe: down, online: false }).verdict).toBe("offline");
    expect(check({ probe: down }).verdict).toBe("unreachable");
    expect(check({ probe: { ok: false, reason: "timeout" } }).verdict).toBe(
      "unreachable",
    );
    expect(check({ probe: { ok: false, reason: "not_relay" } }).verdict).toBe(
      "odd",
    );
  });

  it("浏览器说没联网、但站点其实通了，以通了为准", () => {
    expect(check({ online: false }).verdict).toBe("fine");
  });

  it("站点没通时登录看不了，不说登录有问题", () => {
    const report = check({
      probe: { ok: false, reason: "timeout" },
      status: "session_expired",
    });
    expect(report.login).toBe("skipped");
    expect(report.action).toBe("again");
  });

  it("登录失效让人重新连一次", () => {
    for (const status of ["session_expired", "signed_out", "denied"] as const)
      expect(check({ status })).toMatchObject({
        verdict: "expired",
        action: "connect",
      });
  });

  it("密码库打不开、没读到账户，各说各的，都是再试一次", () => {
    expect(check({ status: "secure_storage_unavailable" })).toMatchObject({
      verdict: "locked",
      action: "again",
    });
    expect(check({ status: "invalid_response" })).toMatchObject({
      verdict: "failed",
      action: "again",
    });
    expect(check({ status: undefined }).verdict).toBe("failed");
  });

  it("有应用要修就指出是哪几个", () => {
    expect(
      check({
        connections: [
          app("claude_code", "connected"),
          app("codex_desktop", "changed"),
          app("pi", "recovery_pending"),
        ],
      }),
    ).toMatchObject({
      apps: "repair",
      repair: ["codex_desktop", "pi"],
      verdict: "repair",
      action: "repair",
    });
  });

  it("一个都没接：让人去接入", () => {
    expect(
      check({ connections: [app("claude_code", "not_connected")] }),
    ).toMatchObject({ apps: "none", verdict: "none", action: "join" });
  });

  it("应用的情况读不到时说看不到，不说没接", () => {
    expect(check({ connectionsFailed: true })).toMatchObject({
      apps: "unknown",
      verdict: "fine",
    });
    expect(check({ connections: [] }).apps).toBe("unknown");
    expect(check({ connections: [app("pi", "unavailable")] }).apps).toBe(
      "unknown",
    );
  });
});

describe("originOfLine", () => {
  it("认得内置线路和带地址的线路", () => {
    expect(originOfLine("mainland_optimized")).toBe("https://yeschoy.com");
    expect(originOfLine("station_key:https://a.example")).toBe(
      "https://a.example",
    );
    expect(originOfLine("station_connect:https://b.example")).toBe(
      "https://b.example",
    );
    expect(originOfLine("station_key:http://a.example")).toBeNull();
    expect(originOfLine("")).toBeNull();
  });
});
