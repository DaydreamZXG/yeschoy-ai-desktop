import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import zh from "../i18n/locales/zh.json";
import { ModelCapabilityBadges } from "./ModelCapabilityBadges";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeAll(async () => {
  await i18n.init({
    lng: "zh",
    resources: { zh: { translation: zh } },
  });
});

afterEach(cleanup);

function badges(): string[] {
  return Array.from(document.querySelectorAll(".model-capability-badge")).map(
    (el) => el.textContent ?? "",
  );
}

describe("ModelCapabilityBadges", () => {
  it("renders the tool-use badge for a verified model", () => {
    render(<ModelCapabilityBadges id="gpt-6-astra" />);
    expect(badges()).toContain("工具调用");
  });

  it("never renders a tool-use badge for the routing alias we do not declare", () => {
    render(<ModelCapabilityBadges id="ark-code-latest" />);
    expect(badges()).not.toContain("工具调用");
  });

  it("renders nothing for an unknown model", () => {
    const { container } = render(<ModelCapabilityBadges id="not-a-model" />);
    expect(container.querySelector(".model-capability-badges")).toBeNull();
  });

  it("renders contiguous reasoning ladders as first–last plus the default", () => {
    render(<ModelCapabilityBadges id="gpt-6-astra" />);
    const list = badges();
    // low/medium/high/xhigh/max 是连续阶梯 → low–max
    expect(list).toContain("思考 low–max");
  });

  it("renders non-contiguous levels explicitly instead of implying a range", () => {
    // deepseek-v4-flash: none/low/high/max 缺 medium → 明列
    render(<ModelCapabilityBadges id="deepseek-v4-flash" />);
    expect(badges()).toContain("思考 none·low·high·max");
    expect(badges()).toContain("默认思考 high");
  });

  it("renders the 1M context badge for million-token models", () => {
    render(<ModelCapabilityBadges id="claude-sonnet-4-6" />);
    expect(badges()).toContain("1M 上下文");
  });

  it("renders a scaled context badge for mid-size windows instead of nothing", () => {
    // gpt-5.4: 272,000 → 272K 上下文；deepseek-r1: 128,000 → 128K
    render(<ModelCapabilityBadges id="gpt-5.4" />);
    expect(badges()).toContain("272K 上下文");
    cleanup();
    render(<ModelCapabilityBadges id="deepseek-r1" />);
    expect(badges()).toContain("128K 上下文");
    expect(badges()).not.toContain("1M 上下文");
  });

  it("shows no reasoning badge when the model has no reasoning levels", () => {
    // qwen3-coder-plus reasoningLevels 为空数组
    render(<ModelCapabilityBadges id="qwen3-coder-plus" />);
    const list = badges();
    expect(list.some((badge) => badge.startsWith("思考"))).toBe(false);
  });

  it("exposes the badge row to assistive tech via aria-label", () => {
    render(<ModelCapabilityBadges id="kimi-k3" />);
    expect(
      screen.getByLabelText("model-capabilities", { exact: false }),
    ).toBeTruthy();
  });
});
