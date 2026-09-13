import { useEffect, useMemo, useState } from "react";

// 首次接入庆祝（动效优化）：彩带只播一次、约 1.2s，ticker 打字机常驻。
// reduced-motion 由全局拍平兜底（confetti 静止瞬间消失），无需逐类处理。
const PARTICLE_COLORS = [
  "var(--accent)",
  "var(--success)",
  "var(--warning)",
  "var(--danger)",
];

function randomIn(min: number, max: number): number {
  return min + Math.random() * (max - min);
}

/** 彩带层：一次性、装饰性（aria-hidden），结束后自动卸载。 */
export function CelebrationConfetti() {
  const [alive, setAlive] = useState(true);
  const particles = useMemo(
    () =>
      Array.from({ length: 40 }, (_, index) => ({
        id: index,
        left: randomIn(0, 100),
        drift: randomIn(-60, 60),
        delay: randomIn(0, 140),
        duration: randomIn(900, 1300),
        size: randomIn(5, 9),
        color: PARTICLE_COLORS[index % PARTICLE_COLORS.length],
        rotate: randomIn(-180, 180),
      })),
    [],
  );
  useEffect(() => {
    const timer = window.setTimeout(() => setAlive(false), 1500);
    return () => window.clearTimeout(timer);
  }, []);
  if (!alive) return null;
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
            transform: `rotate(${p.rotate}deg)`,
            // CSS 变量传漂移量，keyframes 里 var(--drift) 消费
            ["--drift" as string]: `${p.drift}px`,
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
