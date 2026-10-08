import {
  useEffect,
  useLayoutEffect,
  useRef,
  type HTMLAttributes,
  type ReactNode,
} from "react";

/**
 * 动效工具（第四轮「情绪价值」）。全部只动 transform / opacity，
 * 用 WAAPI 与 rAF，不引第三方库。
 *
 * 只有在真浏览器里、且用户没要求减少动效时才播。jsdom 没有
 * `Element.prototype.animate`，所以单测里这些函数全部是空操作，
 * DOM 文本从第一帧起就是最终值。
 */
export function motionAllowed(): boolean {
  if (typeof window === "undefined" || typeof Element === "undefined")
    return false;
  if (typeof Element.prototype.animate !== "function") return false;
  try {
    return !window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  } catch {
    return true;
  }
}

const LIME = "#c9fa10";
const INK = "#111111";
const PAPER = "#ffffff";

/**
 * 成功庆祝：从 origin（默认视口中下部）炸开一圈青柠/黑/白彩带，
 * 中间画一个会「写出来」的对勾。挂在 <body> 上，不受任何祖先 transform 影响，
 * 1.8 秒后自行移除。装饰层 aria-hidden；成功的文字状态由页面本身负责播报。
 */
export function celebrate(origin?: Element | null, big = true): void {
  if (!motionAllowed() || typeof document === "undefined") return;
  const rect = origin?.getBoundingClientRect();
  const x = rect ? rect.left + rect.width / 2 : window.innerWidth / 2;
  const y = rect ? rect.top + rect.height / 2 : window.innerHeight * 0.6;

  const layer = document.createElement("div");
  layer.className = "celebration-layer";
  layer.setAttribute("aria-hidden", "true");
  layer.dataset.testid = "celebration-layer";
  document.body.appendChild(layer);

  const count = big ? 90 : 28;
  const colors = [LIME, LIME, INK, PAPER];
  for (let i = 0; i < count; i++) {
    const p = document.createElement("span");
    p.className = "celebration-bit";
    const size = 6 + Math.random() * (big ? 8 : 5);
    const round = Math.random() < 0.35;
    p.style.cssText =
      `left:${x}px;top:${y}px;width:${size}px;height:${round ? size : size * 0.5}px;` +
      `background:${colors[i % colors.length]};border-radius:${round ? "50%" : "2px"}`;
    layer.appendChild(p);
    const angle = (Math.PI * 2 * i) / count + Math.random() * 0.4;
    const speed = (big ? 220 : 110) + Math.random() * (big ? 260 : 90);
    const dx = Math.cos(angle) * speed;
    const dy = Math.sin(angle) * speed - (big ? 160 : 60);
    const spin = (Math.random() - 0.5) * 900;
    p.animate(
      [
        {
          transform: "translate(-50%,-50%) scale(0.2) rotate(0deg)",
          opacity: 1,
        },
        {
          transform: `translate(calc(-50% + ${dx * 0.75}px), calc(-50% + ${dy * 0.75}px)) scale(1) rotate(${spin * 0.6}deg)`,
          opacity: 1,
          offset: 0.45,
        },
        {
          transform: `translate(calc(-50% + ${dx}px), calc(-50% + ${dy + (big ? 380 : 140)}px)) scale(0.8) rotate(${spin}deg)`,
          opacity: 0,
        },
      ],
      {
        duration: (big ? 1300 : 800) + Math.random() * 400,
        easing: "cubic-bezier(0.2, 0.7, 0.3, 1)",
        fill: "forwards",
      },
    );
  }

  if (big) {
    const check = document.createElement("div");
    check.className = "celebration-check";
    // 对勾放在视口中上部，不压在浮动条上；彩带仍从按钮炸开。
    check.style.left = `${window.innerWidth / 2 + 40}px`;
    check.style.top = `${window.innerHeight * 0.42}px`;
    check.innerHTML =
      '<svg viewBox="0 0 96 96" width="96" height="96"><circle cx="48" cy="48" r="44"/><path d="M28 50 L42 64 L69 34"/></svg>';
    layer.appendChild(check);
    check.animate(
      [
        { transform: "translate(-50%,-50%) scale(0.3)", opacity: 0 },
        {
          transform: "translate(-50%,-50%) scale(1.12)",
          opacity: 1,
          offset: 0.35,
        },
        {
          transform: "translate(-50%,-50%) scale(1)",
          opacity: 1,
          offset: 0.55,
        },
        {
          transform: "translate(-50%,-50%) scale(1)",
          opacity: 1,
          offset: 0.85,
        },
        { transform: "translate(-50%,-50%) scale(0.9)", opacity: 0 },
      ],
      {
        duration: 1700,
        easing: "cubic-bezier(0.34, 1.4, 0.64, 1)",
        fill: "forwards",
      },
    );
  }

  window.setTimeout(() => layer.remove(), 2000);
}

/**
 * 数字从 0 数到目标值。React 渲染的就是最终文本；这里只在浏览器里
 * 临时改写同一个文本节点的 nodeValue，结束时写回原值 —— React 持有的
 * 节点引用不变，之后的更新照常生效，测试看到的永远是最终值。
 */
export function useCountUp<T extends HTMLElement>(
  value: string,
  duration = 900,
) {
  const ref = useRef<T>(null);
  useLayoutEffect(() => {
    const node = ref.current?.firstChild;
    if (!node || node.nodeType !== Node.TEXT_NODE || !motionAllowed()) return;
    const final = node.nodeValue ?? "";
    const match = final.match(/^(\D*?)(\d[\d,]*)(\.\d+)?(.*)$/);
    if (!match) return;
    const [, prefix, whole, fraction = "", suffix] = match;
    const target = Number(`${whole.replace(/,/g, "")}${fraction}`);
    if (!Number.isFinite(target) || target === 0) return;
    const decimals = fraction ? fraction.length - 1 : 0;
    const grouped = whole.includes(",");
    const format = (n: number) => {
      const fixed = n.toFixed(decimals);
      if (!grouped) return fixed;
      const [i, f] = fixed.split(".");
      return i.replace(/\B(?=(\d{3})+(?!\d))/g, ",") + (f ? `.${f}` : "");
    };
    let frame = 0;
    const start = performance.now();
    const tick = (now: number) => {
      const t = Math.min(1, (now - start) / duration);
      const eased = 1 - Math.pow(1 - t, 3);
      node.nodeValue =
        t >= 1 ? final : `${prefix}${format(target * eased)}${suffix}`;
      if (t < 1) frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(frame);
      node.nodeValue = final;
    };
  }, [value, duration]);
  return ref;
}

/** 一个会数数的 <strong>：第一个子节点必须是数字文本。 */
export function CountUpStrong({
  value,
  children,
  ...rest
}: { value: string; children: ReactNode } & HTMLAttributes<HTMLElement>) {
  const ref = useCountUp<HTMLElement>(value);
  return (
    <strong ref={ref} {...rest}>
      {children}
    </strong>
  );
}

/** 卡片上的光标聚光灯：一个全局 pointermove，把坐标写进 CSS 变量。 */
const SPOTLIGHT =
  ".account-overview, .billing-plan-card, .announcement-card, .connection-row, .usage-trend";
export function installSpotlight(): void {
  if (!motionAllowed()) return;
  let pending: { el: HTMLElement; x: number; y: number } | null = null;
  document.addEventListener(
    "pointermove",
    (event) => {
      const el = (event.target as Element | null)?.closest?.(SPOTLIGHT);
      if (!(el instanceof HTMLElement)) return;
      const r = el.getBoundingClientRect();
      if (!pending)
        requestAnimationFrame(() => {
          if (!pending) return;
          pending.el.style.setProperty("--mx", `${pending.x}px`);
          pending.el.style.setProperty("--my", `${pending.y}px`);
          pending = null;
        });
      pending = { el, x: event.clientX - r.left, y: event.clientY - r.top };
    },
    { passive: true },
  );
}

/**
 * 状态从「否」变成「是」的那一刻放一次庆祝。挂载时就已经是「是」的不算
 * —— 那是旧结果，不是刚刚发生的成功。
 */
export function useCelebrateOnRise(
  flag: boolean,
  originSelector?: string,
  big = true,
): void {
  const previous = useRef(flag);
  useEffect(() => {
    if (flag && !previous.current)
      celebrate(
        originSelector ? document.querySelector(originSelector) : null,
        big,
      );
    previous.current = flag;
  }, [flag, originSelector, big]);
}
