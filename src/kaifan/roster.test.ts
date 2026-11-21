import { beforeEach, describe, expect, it } from "vitest";
import {
  ROSTER,
  ROSTER_IDS,
  characterOfModel,
  chibiOf,
  chooseCharacter,
  groupModels,
  portraitOf,
  readPickedCharacter,
  storePickedCharacter,
} from "./roster";

describe("从模型 id 认出牌子", () => {
  it.each([
    ["claude-sonnet-4-5", "cla"],
    ["anthropic/claude-opus-4.1", "cla"],
    ["gpt-5", "gpt"],
    ["o3-mini", "gpt"],
    ["openai/o1", "gpt"],
    ["gpt-5-codex", "gpt"],
    ["gemini-2.5-pro", "gem"],
    ["grok-4", "grk"],
    ["deepseek-chat", "ds"],
    ["qwen-max", "qw"],
    ["kimi-k2", "kimi"],
    ["moonshot-v1-128k", "kimi"],
    ["glm-4-plus", "glm"],
    ["doubao-pro-32k", "dou"],
  ])("%s → %s", (model, id) => {
    expect(characterOfModel(model)).toBe(id);
  });

  it("认不出的牌子归转校生，而不是硬塞给某个人", () => {
    expect(characterOfModel("minimax-m2")).toBe("new");
    expect(characterOfModel("llama-3.3-70b")).toBe("new");
    // 名字里碰巧带 o3 的别家模型不算 GPT。
    expect(characterOfModel("yi-turbo3-large")).toBe("new");
  });

  it("按角色归拢站点目录", () => {
    expect(
      groupModels([
        { id: "deepseek-chat" },
        { id: "deepseek-reasoner" },
        { id: "claude-sonnet-4-5" },
        { id: "minimax-m2" },
      ]),
    ).toEqual({
      ds: ["deepseek-chat", "deepseek-reasoner"],
      cla: ["claude-sonnet-4-5"],
      new: ["minimax-m2"],
    });
  });
});

describe("首页由谁出场", () => {
  const available = groupModels([{ id: "deepseek-chat" }, { id: "gpt-5" }]);

  it("用户点过的人优先，只要这个站还有她", () => {
    expect(
      chooseCharacter({
        picked: "ds",
        available,
        connectedModelIds: ["gpt-5"],
        fallback: "gpt",
      }),
    ).toBe("ds");
  });

  it("点过的人这个站没有，就用已接入应用正在用的那个", () => {
    expect(
      chooseCharacter({
        picked: "cla",
        available,
        connectedModelIds: ["deepseek-chat"],
        fallback: "gpt",
      }),
    ).toBe("ds");
  });

  it("什么都没接，就是这个站有的第一个人", () => {
    expect(
      chooseCharacter({
        picked: null,
        available,
        connectedModelIds: [],
        fallback: "cla",
      }),
    ).toBe("gpt");
  });

  it("目录还不知道时不乱猜，用兜底的人", () => {
    expect(
      chooseCharacter({
        picked: null,
        available: null,
        connectedModelIds: ["deepseek-chat"],
        fallback: "gpt",
      }),
    ).toBe("gpt");
  });
});

describe("记住用户点过的人", () => {
  beforeEach(() => localStorage.clear());

  it("存了能读回来；乱写的值当没存", () => {
    expect(readPickedCharacter()).toBeNull();
    storePickedCharacter("kimi");
    expect(readPickedCharacter()).toBe("kimi");
    localStorage.setItem("kaifan.character", "nobody");
    expect(readPickedCharacter()).toBeNull();
  });
});

describe("素材齐全", () => {
  it("每个人都有立绘和五个表情", () => {
    for (const id of ROSTER_IDS) {
      expect(portraitOf(id)).not.toBe("");
      for (const mood of ["idle", "happy", "hungry", "panic", "sleep"] as const)
        expect(chibiOf(id, mood)).not.toBe("");
      expect(ROSTER[id].brand).not.toBe("");
    }
    expect(portraitOf("new")).not.toBe("");
    expect(chibiOf("new", "panic")).toBe(chibiOf("new", "idle"));
  });
});
