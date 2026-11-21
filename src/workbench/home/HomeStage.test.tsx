import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountModel } from "../../account/session";
import { HomeStage } from "./HomeStage";

const model = (id: string) => ({ id }) as AccountModel;

function stage(overrides: Partial<Parameters<typeof HomeStage>[0]> = {}) {
  const props = {
    models: [
      model("deepseek-chat"),
      model("deepseek-reasoner"),
      model("gpt-5"),
    ],
    connectedModelIds: [],
    connectedCount: 0,
    balance: "¥86.40",
    station: "connected" as const,
    notice: null,
    apps: [
      {
        id: "claude_desktop" as const,
        name: "Claude Desktop",
        label: "Claude Desktop",
        connected: true,
      },
      {
        id: "codex_desktop" as const,
        name: "Codex",
        label: "Codex",
        connected: false,
      },
    ],
    onOpenSetup: vi.fn(),
    onStart: vi.fn(),
    onShowApps: vi.fn(),
    onSignIn: vi.fn(),
    ...overrides,
  };
  render(<HomeStage {...props} />);
  return props;
}

const strip = (name: string) => screen.getByRole("button", { name });

describe("首页舞台", () => {
  beforeEach(() => localStorage.clear());

  it("九个人都在场，站点有的第一个人出场，用真实余额说话", () => {
    stage();
    expect(
      within(screen.getByTestId("home-stage")).getAllByRole("button", {
        pressed: false,
      }).length + 1,
    ).toBe(9);
    expect(strip("阿青（GPT）")).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText(/余额还有 ¥86\.40/)).toBeInTheDocument();
    expect(screen.getByText(/还没接应用/)).toBeInTheDocument();
  });

  it("这个站没有的人留在原位但是暗的，点她不会换人", () => {
    stage();
    const missing = strip("橘子（Claude）");
    expect(missing).toHaveAttribute("data-missing", "true");
    fireEvent.click(missing);
    expect(strip("阿青（GPT）")).toHaveAttribute("aria-pressed", "true");
    expect(strip("小鲸（DeepSeek）")).not.toHaveAttribute("data-missing");
  });

  it("点谁换成谁，并且记住；她在这个站上有几个型号照实写", () => {
    stage();
    fireEvent.click(strip("小鲸（DeepSeek）"));
    expect(strip("小鲸（DeepSeek）")).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText(/现在由我陪你/)).toBeInTheDocument();
    expect(screen.getByText(/等 2 个型号/)).toBeInTheDocument();
    expect(localStorage.getItem("yeschoy-home-character")).toBe("ds");
  });

  it("已接入的应用正在用谁的模型，就是谁出场", () => {
    stage({ connectedModelIds: ["deepseek-chat"], connectedCount: 1 });
    expect(strip("小鲸（DeepSeek）")).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText(/锅里还有 ¥86\.40 的饭/)).toBeInTheDocument();
    expect(screen.getByText(/已经接好 1 个应用/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "再接一个" })).toBeEnabled();
  });

  it("余额看不到就照实说，不编数字", () => {
    stage({ balance: null });
    expect(screen.getByText(/余额这会儿看不到/)).toBeInTheDocument();
    expect(screen.queryByText(/¥/)).toBeNull();
  });

  it("接入状态没读到时不报数，不说「还没接应用」", () => {
    stage({ connectedCount: null });
    expect(screen.queryByText(/还没接应用/)).toBeNull();
    expect(screen.queryByText(/已经接好/)).toBeNull();
  });

  it("认不出牌子的模型才多出一条转校生", () => {
    stage({ models: [model("gpt-5"), model("minimax-m2")] });
    expect(strip("转校生")).not.toHaveAttribute("data-missing");
  });

  it("出状况时换表情，原来的提示搬进对话框里说", () => {
    stage({
      connectedModelIds: ["deepseek-chat"],
      notice: {
        mood: "hungry",
        flavor: "low",
        node: <p role="status">余额不足</p>,
      },
    });
    const root = screen.getByTestId("home-stage");
    expect(root).toHaveAttribute("data-mood", "hungry");
    expect(within(root).getByRole("status")).toHaveTextContent("余额不足");
    // 小鲸有自己的说法；主按钮让位给提示自己的按钮。
    expect(screen.getByText(/喂点米吧/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "开始吧" })).toBeNull();
  });

  it("没连站点时由阿青迎新，不显示应用和余额", () => {
    const props = stage({
      models: null,
      station: "signed_out",
      balance: null,
      connectedCount: null,
    });
    expect(strip("阿青（GPT）")).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText(/先连上你的站点/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Codex/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "去填网址" }));
    expect(props.onSignIn).toHaveBeenCalled();
  });

  it("应用格子：接好的去调整，没接的去接入", () => {
    const props = stage();
    fireEvent.click(
      screen.getByRole("button", { name: /Claude Desktop：已接入/ }),
    );
    expect(props.onOpenSetup).toHaveBeenLastCalledWith(
      "claude_desktop",
      "change-model",
    );
    fireEvent.click(screen.getByRole("button", { name: /Codex：还没接入/ }));
    expect(props.onOpenSetup).toHaveBeenLastCalledWith(
      "codex_desktop",
      "configure",
    );
    fireEvent.click(screen.getByRole("button", { name: "开始吧" }));
    expect(props.onStart).toHaveBeenCalled();
  });
});
