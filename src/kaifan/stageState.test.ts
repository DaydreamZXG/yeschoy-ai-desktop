import { describe, expect, it } from "vitest";
import type { AccountModel } from "../account/session";
import type { ToolConnection } from "../configuration/connections";
import { deriveStage, standIn, workerOf, type StageInput } from "./stageState";

const model = (id: string) => ({ id }) as AccountModel;
const tool = (
  toolId: ToolConnection["toolId"],
  state: ToolConnection["state"],
  modelId = "",
) => ({ toolId, state, modelId }) as ToolConnection;

const base: StageInput = {
  status: "signed_in",
  loading: false,
  stationCheck: "ok",
  money: {
    currency: "CNY",
    balanceAmount: "86.40",
    consumedAmount: "0",
    displayRate: "1",
  },
  models: [model("claude-sonnet-4-5"), model("gpt-5")],
  connections: [
    tool("claude_desktop", "connected", "claude-sonnet-4-5"),
    tool("codex_desktop", "not_connected"),
  ],
  connectionsFailed: false,
  picked: null,
};
const cond = (patch: Partial<StageInput>) =>
  deriveStage({ ...base, ...patch }).cond;

describe("首页一次只演最要紧的一件事", () => {
  it("一切正常", () => {
    const state = deriveStage(base);
    expect(state.cond).toBe("ok");
    expect(state.connectedCount).toBe(1);
    expect(state.balance).toContain("86.40");
  });

  it("画图的型号不算在她名下：首页点的型号数和名片上列的是同一份", () => {
    const state = deriveStage({
      ...base,
      models: [model("gpt-5"), model("gpt-image-2")],
    });
    expect(state.cast?.gpt).toEqual(["gpt-5"]);
  });

  it("没连站点", () => {
    expect(cond({ status: "signed_out" })).toBe("out");
    expect(cond({ status: "session_expired" })).toBe("out");
  });

  it("站点连不上排在余额和模型前面", () => {
    expect(cond({ stationCheck: "unreachable" })).toBe("off");
    expect(cond({ status: "network_error" })).toBe("off");
    expect(cond({ status: "backend_unavailable" })).toBe("off");
  });

  it("账户没读回来、手里也没有上次的结果：算连不上，不一直转圈", () => {
    expect(cond({ status: undefined, failed: true })).toBe("off");
    expect(cond({ status: undefined, failed: true, loading: true })).toBe(
      "checking",
    );
    expect(cond({ status: undefined })).toBe("checking");
  });

  it("余额不足排在模型下线前面", () => {
    const low = { ...base.money!, balanceAmount: "0.50" };
    const state = deriveStage({
      ...base,
      money: low,
      connections: [tool("claude_desktop", "connected", "gone-model")],
    });
    expect(state.cond).toBe("low");
    expect(state.mood).toBe("hungry");
    expect(state.lowAmount).toContain("0.50");
  });

  it("接好的应用用的模型这个站没有了", () => {
    const state = deriveStage({
      ...base,
      connections: [tool("claude_desktop", "connected", "claude-opus-9")],
    });
    expect(state.cond).toBe("busy");
    expect(state.offline).toEqual([
      { toolId: "claude_desktop", modelId: "claude-opus-9" },
    ]);
  });

  it("设置被别的工具改过、或者上次没做完的：不算接好，要先修", () => {
    const state = deriveStage({
      ...base,
      connections: [
        tool("claude_desktop", "changed", "claude-sonnet-4-5"),
        tool("codex_desktop", "recovery_pending", "gpt-5"),
      ],
    });
    expect(state.cond).toBe("fix");
    expect(state.connectedCount).toBe(0);
    expect(state.repair).toEqual([
      { toolId: "claude_desktop", unfinished: false },
      { toolId: "codex_desktop", unfinished: true },
    ]);
    expect(
      workerOf(tool("claude_desktop", "changed", "claude-sonnet-4-5")),
    ).toBeNull();
  });

  it("余额不足排在要修的应用前面", () => {
    expect(
      cond({
        money: { ...base.money!, balanceAmount: "0" },
        connections: [tool("claude_desktop", "changed", "gpt-5")],
      }),
    ).toBe("low");
  });

  it("模型列表还没读到时，不说任何人请假", () => {
    expect(
      cond({
        models: [],
        connections: [tool("claude_desktop", "connected", "claude-opus-9")],
      }),
    ).toBe("ok");
  });
});

describe("不知道的不说成 0", () => {
  it("接入状态没读到", () => {
    expect(deriveStage({ ...base, connections: [] }).connectedCount).toBeNull();
    expect(
      deriveStage({ ...base, connections: undefined }).connectedCount,
    ).toBeNull();
  });

  it("只读到一部分、且没有一个确定接好的", () => {
    expect(
      deriveStage({
        ...base,
        connections: [
          tool("claude_desktop", "unavailable"),
          tool("codex_desktop", "not_connected"),
        ],
      }).connectedCount,
    ).toBeNull();
  });

  it("读到一部分、有确定接好的：数照报，但标明只是「至少」", () => {
    const state = deriveStage({
      ...base,
      connections: [
        tool("claude_desktop", "connected", "claude-sonnet-4-5"),
        tool("codex_desktop", "unavailable"),
      ],
    });
    expect(state.connectedCount).toBe(1);
    expect(state.connectedPartial).toBe(true);
    expect(deriveStage(base).connectedPartial).toBe(false);
  });

  it("确实一个都没接，是 0", () => {
    expect(
      deriveStage({
        ...base,
        connections: [tool("claude_desktop", "not_connected")],
      }).connectedCount,
    ).toBe(0);
  });

  it("余额读不到是 null；没连站点时不拿旧余额出来说", () => {
    expect(deriveStage({ ...base, money: undefined }).balance).toBeNull();
    expect(deriveStage({ ...base, status: "signed_out" }).balance).toBeNull();
    expect(deriveStage({ ...base, status: "signed_out" }).cast).toBeNull();
  });

  it("这次没读到、用的是上次的结果", () => {
    expect(
      deriveStage({ ...base, connectionsFailed: true }).connectedStale,
    ).toBe(true);
    expect(
      deriveStage({ ...base, connectionsFailed: true, connections: [] })
        .connectedStale,
    ).toBe(false);
  });
});

describe("谁在台上", () => {
  it("没连站点时由阿青迎新", () => {
    expect(
      deriveStage({ ...base, status: "signed_out", connections: [] }).current,
    ).toBe("gpt");
  });

  it("点过的人在这个站上就用她", () => {
    expect(deriveStage({ ...base, picked: "gpt" }).current).toBe("gpt");
  });

  it("顶班的人：这个站上有的、不是她自己、也不是转校生", () => {
    expect(standIn({ cla: ["a"], gpt: ["b"] }, "cla")).toBe("gpt");
    expect(standIn({ cla: ["a"], new: ["x"] }, "cla")).toBeNull();
    expect(standIn(null, "cla")).toBeNull();
  });

  it("应用现在是谁在干活", () => {
    expect(workerOf(tool("claude_desktop", "connected", "gpt-5"))).toBe("gpt");
    expect(
      workerOf(tool("claude_desktop", "not_connected", "gpt-5")),
    ).toBeNull();
    expect(workerOf(tool("claude_desktop", "connected"))).toBeNull();
    expect(workerOf(undefined)).toBeNull();
  });
});

describe("换了站点以后", () => {
  const at = (lineId: string) =>
    ({
      ...tool("claude_desktop", "connected", "claude-sonnet-4-5"),
      lineId,
    }) as ToolConnection;
  const other = "station_key:https://other.example.com";

  it("接在原来那个站的应用，不算在现在这个站头上，哪怕型号同名", () => {
    const state = deriveStage({
      ...base,
      connections: [at("mainland_optimized")],
      lineId: other,
    });
    expect(state.connectedCount).toBe(0);
    expect(state.offline).toEqual([]);
    expect(workerOf(at("mainland_optimized"), other)).toBeNull();
  });

  it("同一个站的两种写法算同一个站；没记站点的旧接入不猜", () => {
    expect(
      deriveStage({
        ...base,
        connections: [at("mainland_optimized")],
        lineId: "station_connect:https://yeschoy.com",
      }).connectedCount,
    ).toBe(1);
    expect(
      deriveStage({ ...base, connections: [at("")], lineId: other })
        .connectedCount,
    ).toBe(1);
    expect(workerOf(at(other), other)).toBe("cla");
  });
});
