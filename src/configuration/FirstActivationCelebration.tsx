import { useEffect, useMemo, useState } from "react";

// 首次接入庆祝（动效优化）：彩带只播一次，ticker 打字机常驻。
const PARTICLE_COLORS = [
  "var(--accent)",
  "var(--success)",
  "var(--warning)",
  "var(--danger)",
];

function randomIn(min: number, max: number): number {
  return min + Math.random() * (max - min);
}

/** 用户在系统里要求减少动效。装饰性动画应该整个不做，而不是拍平成静止的 DOM。 */
function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

/** 彩带层：一次性、装饰性（aria-hidden），结束后自动卸载。 */
export function CelebrationConfetti() {
  const [alive, setAlive] = useState(true);
  const reducedMotion = useMemo(prefersReducedMotion, []);
  const particles = useMemo(
    () =>
      reducedMotion
        ? []
        : Array.from({ length: 40 }, (_, index) => ({
            id: index,
            left: randomIn(0, 100),
            drift: randomIn(-60, 60),
            delay: randomIn(0, 140),
            duration: randomIn(900, 1300),
            size: randomIn(5, 9),
            color: PARTICLE_COLORS[index % PARTICLE_COLORS.length],
            rotate: randomIn(-180, 180),
          })),
    [reducedMotion],
  );
  // 卸载时间从粒子本身算出来，而不是写死。原来是固定 1500ms，而最慢的一片
  // 是 140ms 延迟 + 1300ms 时长 = 1440ms —— 只剩 60ms 余量，而且两个计时的
  // 起点还不一样（动画从首帧开始，定时器从 effect 开始）。首帧一慢，彩带就
  // 在半空中被整层删掉，而不是落到底淡出。
  const lifetimeMs = useMemo(
    () =>
      particles.reduce(
        (longest, p) => Math.max(longest, p.delay + p.duration),
        0,
      ) + 120,
    [particles],
  );
  useEffect(() => {
    if (!particles.length) {
      setAlive(false);
      return;
    }
    const timer = window.setTimeout(() => setAlive(false), lifetimeMs);
    return () => window.clearTimeout(timer);
  }, [particles.length, lifetimeMs]);
  if (!alive || !particles.length) return null;
  return (
    <div className="celebration-confetti" aria-hidden="true">
      {particles.map((p) => (
        <span
          key={p.id}
          className="celebration-particle"
          style={{
            left: `${p.left}%`,
            width: p.size,
            height: p.size * 0.6,
            background: p.color,
            animationDelay: `${p.delay}ms`,
            animationDuration: `${p.duration}ms`,
            // 漂移和初始旋转都通过 CSS 变量交给 keyframes。
            // 不能在这里写 transform：动画期间 transform 归动画所有，
            // 内联的 rotate() 会被整条覆盖，随机角度等于白写。
            ["--drift" as string]: `${p.drift}px`,
            ["--rotate" as string]: `${p.rotate}deg`,
          }}
        />
      ))}
    </div>
  );
}

/** ticker 打字机：字符逐个浮现；reduced-motion 下无动画即完整显示。 */
export function TickerText({
  text,
  stepMs = 36,
  delayMs = 0,
}: {
  text: string;
  stepMs?: number;
  delayMs?: number;
}) {
  const chars = useMemo(() => Array.from(text), [text]);
  return (
    <span className="ticker-text" aria-label={text}>
      {chars.map((char, index) => (
        <span
          key={`${index}-${char}`}
          className="ticker-char"
          aria-hidden="true"
          style={{ animationDelay: `${delayMs + index * stepMs}ms` }}
        >
          {char === " " ? "\u00A0" : char}
        </span>
      ))}
    </span>
  );
}
