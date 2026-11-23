import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import type { AccountModel } from "../account/session";
import type { ToolConnection } from "../configuration/connections";
import { Card } from "./Card";
import type { CardPicks } from "./cardInfo";

const model = (
  id: string,
  groups: [string, number | null][],
  prices: [string, string] | null,
): AccountModel => ({
  id,
  description: "",
  billingMode: "ratio",
  pricingAvailable: !!prices,
  officialInputCnyPerMillion: prices?.[0] ?? "",
  officialOutputCnyPerMillion: prices?.[1] ?? "",
  actualInputCnyPerMillion: "",
  actualOutputCnyPerMillion: "",
  billing: groups.length
    ? {
        groups: groups.map(([group, ratio]) => ({
          id: group,
          description: "",
          ratio,
        })),
        baseInputUsd: null,
        baseOutputUsd: null,
        requestUsd: null,
        expression: "",
      }
    : null,
});
const SONNET = "claude-sonnet-4-5-20250929";
const OPUS = "claude-opus-4-1-20250805";
const PLANS: [string, number | null][] = [
  ["default", 0.3],
  ["特价", 0.15],
  ["官转", 0.8],
];
const MODELS = [
  model(SONNET, PLANS, ["21", "105"]),
  model(OPUS, [["default", 0.5]], ["105", "525"]),
];
const saved = (patch: Partial<ToolConnection> = {}): ToolConnection => ({
  toolId: "claude_code",
  state: "connected",
  modelId: SONNET,
  lineId: "mainland_optimized",
  billingGroup: "default",
  updatedAtEpochMs: 1,
  restoreMode: "original",
  requiresBackground: false,
  reasonCode: "",
  ...patch,
});

function setup(patch: Partial<Parameters<typeof Card>[0]> = {}) {
  const props = {
    open: true,
    who: "cla" as const,
    ids: [SONNET, OPUS],
    models: MODELS,
    picks: { model: {}, plan: {} } as CardPicks,
    connections: [] as ToolConnection[],
    onModel: vi.fn(),
    onPlan: vi.fn(),
    onJoin: vi.fn(),
    onClose: vi.fn(),
    ...patch,
  };
  render(<Card {...props} />);
  return props;
}
const says = () => screen.getByTestId("kaifan-card-says").textContent;
const option = (name: RegExp) => screen.getByRole("button", { name });

beforeEach(() => {
  vi.clearAllMocks();
  window.matchMedia = vi.fn().mockReturnValue({ matches: true });
});

describe("角色名片", () => {
  it("每个价格方案写成几折，最便宜的那个标出来，她说现在走哪个", () => {
    setup();
    expect(option(/标准分组/)).toHaveTextContent("约 3 折");
    expect(option(/特价/)).toHaveTextContent("约 1.5 折");
    expect(option(/特价/)).toHaveTextContent("最省");
    expect(option(/官转/)).toHaveTextContent("约 8 折");
    expect(option(/标准分组/)).toHaveAttribute("aria-pressed", "true");
    expect(says()).toBe("现在走「标准分组」，约 3 折。想换在下面点就行。");
  });

  it("换型号、换方案都交给外面记住；挑过的下次还是它", () => {
    const props = setup({
      picks: { model: {}, plan: { [SONNET]: "特价" } },
    });
    expect(option(/特价/)).toHaveAttribute("aria-pressed", "true");
    expect(says()).toContain("约 1.5 折");
    fireEvent.click(option(/官转/));
    expect(props.onPlan).toHaveBeenCalledWith(SONNET, "官转");
    fireEvent.click(option(/Opus/i));
    expect(props.onModel).toHaveBeenCalledWith(OPUS);
  });

  it("具体单价：官方价和这个方案的价，都是站点报的数算出来的", () => {
    setup({ picks: { model: {}, plan: { [SONNET]: "特价" } } });
    const prices = screen.getByTestId("kaifan-card-prices");
    expect(prices).toHaveTextContent("¥21.00");
    expect(prices).toHaveTextContent("¥105.00");
    expect(prices).toHaveTextContent("¥3.15");
    expect(prices).toHaveTextContent("¥15.75");
  });

  it("几折怎么算的那句说明：只在名片上真写了几折或几倍时才出现", () => {
    const note = "几折是按站点报的倍率算的，充值时的优惠不算在内。";
    setup();
    expect(screen.getByText(note)).toBeInTheDocument();
    cleanup();
    setup({
      models: [model(OPUS, [["default", 1]], ["105", "525"])],
      picks: { model: { cla: OPUS }, plan: {} },
    });
    expect(option(/标准分组/)).toHaveTextContent("原价");
    expect(screen.queryByText(note)).toBeNull();
  });

  it("只有一个方案：不用挑，也点不了", () => {
    setup({ picks: { model: { cla: OPUS }, plan: {} } });
    expect(says()).toBe("这个站就这一个方案，约 5 折，省得挑了。");
    expect(option(/标准分组/)).toBeDisabled();
    expect(screen.queryByText("最省")).toBeNull();
  });

  it("站点没给官方价：不说几折，只照实写倍率；单价说没有，不写成 0", () => {
    setup({ models: [model(SONNET, PLANS, null)], ids: [SONNET] });
    expect(document.body.textContent).not.toMatch(/折。|约 \d/);
    expect(option(/特价/)).toHaveTextContent("倍率 0.15");
    expect(says()).toContain("算不出几折");
    expect(screen.getByTestId("kaifan-card-prices")).toHaveTextContent(
      "这个站没有提供这个型号的价格。",
    );
    expect(document.body.textContent).not.toContain("¥0");
  });

  it("分档收费的型号：照样说几折，每一档的官方价和这个方案的价都列出来", () => {
    setup({
      models: [
        {
          ...model(SONNET, PLANS, null),
          billingMode: "tiered_expr",
          billing: {
            ...model(SONNET, PLANS, null).billing!,
            expression:
              'len <= 200000 ? tier("standard", p * 2 + c * 6) : tier("long_context", p * 4 + c * 12)',
          },
        },
      ],
      ids: [SONNET],
      fx: "1",
    });
    expect(option(/标准分组/)).toHaveTextContent("约 3 折");
    expect(says()).toBe("现在走「标准分组」，约 3 折。想换在下面点就行。");
    const prices = screen.getByTestId("kaifan-card-prices");
    expect(prices).toHaveTextContent("standard");
    expect(prices).toHaveTextContent("long_context");
    expect(prices).toHaveTextContent("¥2.00");
    expect(prices).toHaveTextContent("¥0.60");
    expect(prices).toHaveTextContent("¥12.00");
    expect(prices).toHaveTextContent("¥3.60");
    expect(document.body.textContent).not.toContain("没有提供这个型号的价格");
  });

  it("按次收费的型号：写每次请求多少钱", () => {
    setup({
      models: [
        {
          ...model(SONNET, [["default", 0.5]], null),
          billingMode: "tiered_expr",
          billing: {
            ...model(SONNET, [["default", 0.5]], null).billing!,
            expression: 'tier("base", fixed(0.16))',
          },
        },
      ],
      ids: [SONNET],
      fx: "1",
    });
    const prices = screen.getByTestId("kaifan-card-prices");
    expect(prices).toHaveTextContent("每次请求");
    expect(prices).toHaveTextContent("¥0.16");
    expect(prices).toHaveTextContent("¥0.08");
  });

  it("方案的倍率站点没给：说没给，这个方案的价写不知道", () => {
    setup({
      models: [model(SONNET, [["default", null]], ["21", "105"])],
      ids: [SONNET],
    });
    expect(option(/标准分组/)).toHaveTextContent("站点没给倍率");
    const prices = screen.getByTestId("kaifan-card-prices");
    expect(prices).toHaveTextContent("¥21.00");
    expect(prices).toHaveTextContent("不知道");
    expect(prices.textContent).not.toContain("¥0");
  });

  it("站点没说有哪些方案：不列方案，也不猜价格", () => {
    setup({ models: [model(SONNET, [], null)], ids: [SONNET] });
    expect(screen.queryByText("价格方案")).toBeNull();
    expect(says()).toContain("我就不瞎猜价了");
  });

  it("她在哪些应用里干活：写的是接入时存下来的型号和方案", () => {
    setup({
      connections: [
        saved(),
        saved({ toolId: "codex_desktop", modelId: "gpt-5" }),
        saved({ toolId: "opencode", state: "not_connected" }),
      ],
    });
    const jobs = screen.getByText("她在哪些应用里干活").parentElement!;
    expect(jobs).toHaveTextContent("Claude Code");
    expect(jobs).toHaveTextContent("标准分组");
    expect(jobs).not.toHaveTextContent("Codex");
    expect(jobs).not.toHaveTextContent("OpenCode");
    expect(screen.queryByText(/再去接入一次/)).toBeNull();
  });

  it("名片上换了方案，接好的应用不会被悄悄改：说清楚要再接入一次", () => {
    setup({
      connections: [saved()],
      picks: { model: {}, plan: { [SONNET]: "特价" } },
    });
    expect(screen.getByText(/再去接入一次/)).toBeInTheDocument();
  });

  it("没在名片上挑过：写的就是接好的那个型号和方案，不说要重新接入", () => {
    setup({
      connections: [saved({ modelId: OPUS })],
      ids: [SONNET, OPUS],
    });
    expect(option(/Opus/)).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryByText(/再去接入一次/)).toBeNull();
  });

  it("接好的应用走的不是默认方案：名片上选中的也是那个，不说要重新接入", () => {
    setup({ connections: [saved({ billingGroup: "官转" })] });
    expect(option(/官转/)).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryByText(/再去接入一次/)).toBeNull();
  });

  it("名片上换了型号：再接入一次真的会换，才提醒", () => {
    setup({
      connections: [saved()],
      picks: { model: { cla: OPUS }, plan: {} },
    });
    expect(screen.getByText(/再去接入一次/)).toBeInTheDocument();
  });

  it("还没接任何应用：照实说；点按钮让她去接入", () => {
    const props = setup();
    expect(screen.getByText("还没接任何应用。")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("kaifan-card-join"));
    expect(props.onJoin).toHaveBeenCalledWith(undefined);
  });

  it("已经在干活：去接入时带上名片说的那个应用，接入屏讲的是同一次接入", () => {
    const props = setup({ connections: [saved()] });
    fireEvent.click(screen.getByTestId("kaifan-card-join"));
    expect(props.onJoin).toHaveBeenCalledWith(saved().toolId);
  });

  it("转校生：没有擅长方向可写，型号和价格照常", () => {
    setup({
      who: "new",
      ids: ["minimax-m2"],
      models: [model("minimax-m2", [["default", 1]], ["2", "8"])],
    });
    expect(screen.queryByText("她擅长")).toBeNull();
    expect(says()).toContain("还没来得及画头像");
    expect(option(/标准分组/)).toHaveTextContent("原价");
  });

  it("按 Esc 或点旁边的空处收起；收起后不接键盘", () => {
    const props = setup();
    const card = screen.getByTestId("kaifan-card");
    expect(card).toContainElement(document.activeElement as HTMLElement);
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    fireEvent.click(card);
    expect(props.onClose).toHaveBeenCalledTimes(2);
  });

  it("没打开时整张名片不接键盘和读屏", () => {
    setup({ open: false });
    const card = screen.getByTestId("kaifan-card");
    expect(card).toHaveAttribute("inert");
  });
});
