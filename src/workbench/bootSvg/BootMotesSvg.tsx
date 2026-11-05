/**
 * Pollen / seed motes drifting through the lucent veil.
 * Soft citrus light, not HUD sparks.
 */
const MOTES: Array<{
  cx: number;
  cy: number;
  r: number;
  delay: string;
  kind: "dot" | "seed";
  rot?: number;
}> = [
  { cx: 10, cy: 24, r: 1.5, delay: "0s", kind: "dot" },
  { cx: 86, cy: 16, r: 1.2, delay: "0.5s", kind: "seed", rot: -28 },
  { cx: 18, cy: 68, r: 1.6, delay: "0.9s", kind: "dot" },
  { cx: 74, cy: 62, r: 1.9, delay: "1.2s", kind: "seed", rot: 18 },
  { cx: 46, cy: 8, r: 1.1, delay: "0.3s", kind: "dot" },
  { cx: 64, cy: 88, r: 1.4, delay: "1.6s", kind: "dot" },
  { cx: 6, cy: 50, r: 1.0, delay: "0.7s", kind: "seed", rot: 42 },
  { cx: 94, cy: 42, r: 1.5, delay: "1.4s", kind: "dot" },
  { cx: 34, cy: 90, r: 1.2, delay: "1.0s", kind: "seed", rot: -12 },
  { cx: 70, cy: 10, r: 1.3, delay: "1.8s", kind: "dot" },
  { cx: 28, cy: 38, r: 0.9, delay: "0.2s", kind: "dot" },
  { cx: 58, cy: 54, r: 1.1, delay: "2.1s", kind: "seed", rot: 8 },
  { cx: 82, cy: 78, r: 1.0, delay: "0.6s", kind: "dot" },
  { cx: 42, cy: 20, r: 1.4, delay: "1.9s", kind: "seed", rot: -36 },
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
        <radialGradient id="qm-mote" cx="50%" cy="50%" r="50%">
          <stop offset="0%" stopColor="#ffffff" stopOpacity="1" />
          <stop offset="40%" stopColor="#c9fa10" stopOpacity="0.9" />
          <stop offset="100%" stopColor="#d4b36a" stopOpacity="0" />
        </radialGradient>
      </defs>
      {MOTES.map((mote, index) =>
        mote.kind === "seed" ? (
          <ellipse
            key={index}
            className="boot-svg-mote"
            cx={mote.cx}
            cy={mote.cy}
            rx={mote.r * 0.7}
            ry={mote.r * 1.35}
            fill="url(#qm-mote)"
            transform={`rotate(${mote.rot ?? 0} ${mote.cx} ${mote.cy})`}
            style={{ animationDelay: mote.delay }}
          />
        ) : (
          <circle
            key={index}
            className="boot-svg-mote"
            cx={mote.cx}
            cy={mote.cy}
            r={mote.r}
            fill="url(#qm-mote)"
            style={{ animationDelay: mote.delay }}
          />
        ),
      )}
    </svg>
  );
}
