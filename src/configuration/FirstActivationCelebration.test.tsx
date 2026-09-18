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

  it("初始旋转角通过 CSS 变量传递，而不是内联 transform", () => {
    // 动画运行期间 transform 完全归 keyframes 所有。以前这里写的是
    // style={{ transform: rotate(...) }}，被 confetti-fall 整条覆盖，
    // 40 片彩带全部同步旋转 —— 随机角度等于没写。
    const { container } = render(<CelebrationConfetti />);
    for (const particle of container.querySelectorAll<HTMLElement>(
      ".celebration-particle",
    )) {
      expect(particle.style.getPropertyValue("--rotate")).toMatch(/^-?[\d.]+deg$/);
      expect(particle.style.transform).toBe("");
    }
  });

  it("卸载时机覆盖最慢的一片，而不是写死 1500ms", () => {
    // 最慢的一片是 140ms 延迟 + 1300ms 时长；固定 1500ms 只剩 60ms 余量，
    // 而且动画从首帧计时、定时器从 effect 计时，起点并不相同。
    vi.useFakeTimers();
    const { container } = render(<CelebrationConfetti />);
    const slowest = Math.max(
      ...[...container.querySelectorAll<HTMLElement>(".celebration-particle")].map(
        (particle) =>
          Number.parseFloat(particle.style.animationDelay) +
          Number.parseFloat(particle.style.animationDuration),
      ),
    );
    act(() => {
      vi.advanceTimersByTime(Math.floor(slowest));
    });
    expect(
      container.querySelector(".celebration-confetti"),
      "最后一片还在下落时不能把整层删掉",
    ).not.toBeNull();
    act(() => {
      vi.advanceTimersByTime(200);
    });
    expect(container.querySelector(".celebration-confetti")).toBeNull();
    vi.useRealTimers();
  });

  it("用户要求减少动效时，整层不渲染", () => {
    // 装饰性动画应该整个不做，而不是拍平成 40 个静止的 DOM 节点。
    const matchMedia = vi
      .spyOn(window, "matchMedia")
      .mockImplementation(
        (query: string) =>
          ({
            matches: query.includes("prefers-reduced-motion"),
            media: query,
            onchange: null,
            addListener: () => {},
            removeListener: () => {},
            addEventListener: () => {},
            removeEventListener: () => {},
            dispatchEvent: () => false,
          }) as MediaQueryList,
      );
    const { container } = render(<CelebrationConfetti />);
    expect(container.querySelector(".celebration-confetti")).toBeNull();
    expect(container.querySelectorAll(".celebration-particle").length).toBe(0);
    matchMedia.mockRestore();
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
