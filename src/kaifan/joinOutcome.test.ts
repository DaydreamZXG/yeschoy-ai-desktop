import { describe, expect, it } from "vitest";
import { joinOutcome, joinStep } from "./joinOutcome";

const of = (status: string, reasonCode = "", skipped?: string[]) =>
  joinOutcome({
    status: status as never,
    reasonCode,
    skipped: skipped?.map((code) => ({
      modelId: "m",
      billingGroup: "default",
      reasonCode: code,
    })),
  });

describe("接入结果怎么说", () => {
  it("成了就是成了", () => {
    expect(of("ready", "configuration_ready")).toMatchObject({
      kind: "done",
      mood: "happy",
    });
  });

  it("设置写好但没能打开应用：不算失败，也不说已经打开", () => {
    expect(of("launch_failed", "terminal_unavailable").kind).toBe("written");
    expect(of("launch_failed", "desktop_state_unavailable").kind).toBe("retry");
  });

  it("应用开着：先问一句再重启；Claude 的登录冲突问的是另一件事", () => {
    expect(of("application_running", "desktop_running")).toMatchObject({
      kind: "askRestart",
      action: "restart",
    });
    expect(of("application_running", "claude_login_conflict")).toMatchObject({
      kind: "askClaudeLogin",
      action: "displace",
    });
    expect(
      of("application_running", "graceful_restart_required"),
    ).toMatchObject({ kind: "closeApp", action: "retry" });
  });

  it("登录不算数了：不管写在哪儿，出路都是重新连站点", () => {
    for (const result of [
      of("signed_out"),
      of("verification_failed", "authentication_failed"),
      of("server_unavailable", "server_unavailable", ["signed_out"]),
    ])
      expect(result).toMatchObject({ kind: "relogin", action: "reconnect" });
  });

  it("站点那边的问题：自己再试，不让用户去查", () => {
    expect(of("server_unavailable").kind).toBe("stationDown");
    expect(of("verification_failed", "provider_busy").kind).toBe("stationDown");
  });

  it("站点没给默认型号的价格方案开通：点名是方案的事，不是笼统的站点没回应", () => {
    const blocked = (modelId: string, reasonCode: string) =>
      joinOutcome({
        status: "server_unavailable",
        reasonCode,
        modelId: "m",
        skipped: [{ modelId, billingGroup: "vip", reasonCode }],
      });
    expect(blocked("m", "server_unavailable").kind).toBe("planBlocked");
    // 被跳过的不是默认型号，或者其实是整次操作被打断：仍然按站点没回应说。
    expect(blocked("other", "server_unavailable").kind).toBe("stationDown");
    expect(blocked("m", "account_changed").kind).toBe("stationDown");
  });

  it("没接成：改回原样了和没改回去是两句话", () => {
    expect(
      of("configuration_failed", "desktop_start_failed_restored"),
    ).toMatchObject({ kind: "restored", action: "retry" });
    expect(
      of("configuration_failed", "configuration_rollback_failed"),
    ).toMatchObject({ kind: "unfinished", action: "classic" });
  });

  it("换个人能解决的，带回去换人", () => {
    expect(of("unsupported_model").action).toBe("pick");
    expect(of("verification_failed", "model_request_rejected").action).toBe(
      "pick",
    );
  });

  it("没装、要选位置、缺环境", () => {
    expect(of("tool_not_found").kind).toBe("notInstalled");
    expect(of("multiple_installations").kind).toBe("chooseInstall");
    expect(of("missing_runtime").kind).toBe("runtime");
  });

  it("认不出的原因：照实说没成，给再试一次", () => {
    expect(of("configuration_failed", "something_new")).toMatchObject({
      kind: "retry",
      action: "retry",
    });
    expect(of("invalid_request").kind).toBe("retry");
  });

  it("用户自己停下的不算出错", () => {
    expect(of("configuration_failed", "activation_cancelled")).toMatchObject({
      kind: "cancelled",
      mood: "idle",
    });
  });
});

describe("进度的三步", () => {
  it("后端的阶段归到三步里", () => {
    expect(joinStep(undefined)).toBe(0);
    expect(joinStep("checking_models")).toBe(0);
    expect(joinStep("applying_settings")).toBe(1);
    expect(joinStep("opening_application")).toBe(2);
    expect(joinStep("complete")).toBe(3);
  });
});
