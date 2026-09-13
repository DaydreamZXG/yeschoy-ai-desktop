import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import {
  CelebrationConfetti,
  TickerText,
} from "./FirstActivationCelebration";

afterEach(cleanup);

describe("CelebrationConfetti", () => {
  it("挂载时渲染彩带层与粒子，aria-hidden", () => {
    const { container } = render(<CelebrationConfetti />);
    const layer = container.querySelector(".celebration-confetti");
    expect(layer).not.toBeNull();
    expect(layer?.getAttribute("aria-hidden")).toBe("true");
    expect(container.querySelectorAll(".celebration-particle").length).toBe(40);
  });

  it("粒子带漂移变量与颜色，落在 token 集合内", () => {
    const { container } = render(<CelebrationConfetti />);
    const particle = container.querySelector<HTMLElement>(
      ".celebration-particle",
    );
    expect(particle?.style.getPropertyValue("--drift")).toMatch(/px$/);
    expect(particle?.style.background).toMatch(/var\(--(accent|success|warning|danger)\)/);
  });

  it("1.5s 后自动卸载", () => {
    vi.useFakeTimers();
    const { container } = render(<CelebrationConfetti />);
    expect(container.querySelector(".celebration-confetti")).not.toBeNull();
    act(() => {
      vi.advanceTimersByTime(1600);
    });
    expect(container.querySelector(".celebration-confetti")).toBeNull();
    vi.useRealTimers();
  });
});

describe("TickerText", () => {
  it("按字符逐个渲染，延迟递增", () => {
    const { container } = render(<TickerText text="接入完成" stepMs={36} />);
    const chars = container.querySelectorAll<HTMLElement>(".ticker-char");
    expect(chars.length).toBe(4);
    expect(chars[0].style.animationDelay).toBe("0ms");
    expect(chars[1].style.animationDelay).toBe("36ms");
    expect(chars[3].style.animationDelay).toBe("108ms");
  });

  it("对外暴露完整 aria-label，字符自身 aria-hidden", () => {
    const { container } = render(<TickerText text="OK" />);
    expect(container.querySelector(".ticker-text")?.getAttribute("aria-label")).toBe("OK");
    for (const el of container.querySelectorAll(".ticker-char")) {
      expect(el.getAttribute("aria-hidden")).toBe("true");
    }
  });

  it("空格转为不换行空格", () => {
    const { container } = render(<TickerText text="a b" stepMs={10} />);
    const chars = container.querySelectorAll<HTMLElement>(".ticker-char");
    expect(chars[1].textContent).toBe("\u00A0");
  });
});
