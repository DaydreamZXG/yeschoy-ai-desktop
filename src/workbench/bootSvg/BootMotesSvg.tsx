/**
 * Kinetic sparks — elongated streaks, not pollen motes.
 */
const STREAKS: Array<{
  x: number;
  y: number;
  len: number;
  angle: number;
  delay: string;
}> = [
  { x: 8, y: 22, len: 14, angle: -28, delay: "0s" },
  { x: 82, y: 14, len: 11, angle: -18, delay: "0.12s" },
  { x: 16, y: 70, len: 16, angle: -34, delay: "0.08s" },
  { x: 72, y: 62, len: 13, angle: -22, delay: "0.2s" },
  { x: 44, y: 8, len: 10, angle: -40, delay: "0.04s" },
  { x: 64, y: 88, len: 15, angle: -16, delay: "0.28s" },
  { x: 6, y: 48, len: 9, angle: -30, delay: "0.16s" },
  { x: 92, y: 40, len: 12, angle: -24, delay: "0.22s" },
  { x: 34, y: 90, len: 11, angle: -36, delay: "0.1s" },
  { x: 70, y: 10, len: 13, angle: -14, delay: "0.32s" },
  { x: 28, y: 36, len: 8, angle: -26, delay: "0.06s" },
  { x: 56, y: 54, len: 17, angle: -20, delay: "0.18s" },
  { x: 86, y: 78, len: 10, angle: -32, delay: "0.26s" },
  { x: 40, y: 18, len: 12, angle: -12, delay: "0.14s" },
];

export function BootMotesSvg() {
  return (
    <svg
      className="boot-svg-motes"
      data-testid="boot-svg-motes"
      viewBox="0 0 100 100"
      preserveAspectRatio="xMidYMid slice"
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        <linearGradient id="fc-streak" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="#ffffff" stopOpacity="0" />
          <stop offset="45%" stopColor="#c9fa10" stopOpacity="1" />
          <stop offset="100%" stopColor="#1cf0ff" stopOpacity="0" />
        </linearGradient>
      </defs>
      {STREAKS.map((streak, index) => (
        <line
          key={index}
          className="boot-svg-mote"
          x1={streak.x}
          y1={streak.y}
          x2={streak.x + Math.cos((streak.angle * Math.PI) / 180) * streak.len}
          y2={streak.y + Math.sin((streak.angle * Math.PI) / 180) * streak.len}
          stroke="url(#fc-streak)"
          strokeWidth="0.7"
          strokeLinecap="round"
          style={{ animationDelay: streak.delay }}
        />
      ))}
    </svg>
  );
}
