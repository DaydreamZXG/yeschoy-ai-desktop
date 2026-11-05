/**
 * Drifting petals and light motes — slow bloom, not kinetic slashes.
 */
const MOTES: Array<{
  x: number;
  y: number;
  r: number;
  kind: "dot" | "petal";
  delay: string;
  rot?: number;
}> = [
  { x: 12, y: 22, r: 1.4, kind: "dot", delay: "0s" },
  { x: 78, y: 16, r: 2.1, kind: "petal", delay: "0.4s", rot: 18 },
  { x: 24, y: 68, r: 1.1, kind: "dot", delay: "1.1s" },
  { x: 86, y: 58, r: 1.8, kind: "petal", delay: "0.2s", rot: -24 },
  { x: 46, y: 10, r: 1.2, kind: "dot", delay: "0.7s" },
  { x: 62, y: 84, r: 2.4, kind: "petal", delay: "1.6s", rot: 40 },
  { x: 8, y: 48, r: 1.6, kind: "petal", delay: "0.9s", rot: -12 },
  { x: 92, y: 38, r: 1.0, kind: "dot", delay: "1.3s" },
  { x: 34, y: 88, r: 1.7, kind: "petal", delay: "0.5s", rot: 8 },
  { x: 70, y: 8, r: 1.3, kind: "dot", delay: "1.8s" },
  { x: 18, y: 36, r: 2.0, kind: "petal", delay: "0.15s", rot: 28 },
  { x: 54, y: 52, r: 1.5, kind: "dot", delay: "1.0s" },
  { x: 88, y: 78, r: 1.9, kind: "petal", delay: "1.4s", rot: -32 },
  { x: 40, y: 28, r: 1.2, kind: "dot", delay: "0.6s" },
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
        <radialGradient id="th-mote-dot" cx="35%" cy="30%" r="70%">
          <stop offset="0%" stopColor="#ffffff" stopOpacity="1" />
          <stop offset="100%" stopColor="#6d5cff" stopOpacity="0.15" />
        </radialGradient>
        <linearGradient id="th-mote-petal" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="#ff7eb3" stopOpacity="0.85" />
          <stop offset="100%" stopColor="#6d5cff" stopOpacity="0.25" />
        </linearGradient>
      </defs>
      {MOTES.map((mote, index) =>
        mote.kind === "petal" ? (
          <ellipse
            key={index}
            className="boot-svg-mote"
            cx={mote.x}
            cy={mote.y}
            rx={mote.r * 0.55}
            ry={mote.r * 1.35}
            fill="url(#th-mote-petal)"
            transform={`rotate(${mote.rot ?? 0} ${mote.x} ${mote.y})`}
            style={{ animationDelay: mote.delay }}
          />
        ) : (
          <circle
            key={index}
            className="boot-svg-mote"
            cx={mote.x}
            cy={mote.y}
            r={mote.r}
            fill="url(#th-mote-dot)"
            style={{ animationDelay: mote.delay }}
          />
        ),
      )}
    </svg>
  );
}
