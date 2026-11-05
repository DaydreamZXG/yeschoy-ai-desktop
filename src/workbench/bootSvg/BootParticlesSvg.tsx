/**
 * Soft spark / particle dots drifting around the HUD.
 * Positions are fixed; CSS animates opacity + translate.
 */
const SPARKS: Array<{ cx: number; cy: number; r: number; delay: string }> = [
  { cx: 12, cy: 28, r: 1.6, delay: "0s" },
  { cx: 88, cy: 18, r: 1.2, delay: "0.4s" },
  { cx: 22, cy: 72, r: 1.4, delay: "0.8s" },
  { cx: 78, cy: 66, r: 1.8, delay: "1.1s" },
  { cx: 48, cy: 8, r: 1.1, delay: "0.2s" },
  { cx: 62, cy: 86, r: 1.3, delay: "1.5s" },
  { cx: 6, cy: 52, r: 1.0, delay: "0.6s" },
  { cx: 94, cy: 44, r: 1.5, delay: "1.3s" },
  { cx: 36, cy: 90, r: 1.2, delay: "0.9s" },
  { cx: 70, cy: 12, r: 1.4, delay: "1.7s" },
];

export function BootParticlesSvg() {
  return (
    <svg
      className="boot-svg-particles"
      data-testid="boot-svg-particles"
      viewBox="0 0 100 100"
      preserveAspectRatio="xMidYMid slice"
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        <radialGradient id="boot-spark" cx="50%" cy="50%" r="50%">
          <stop offset="0%" stopColor="#fff7eb" stopOpacity="1" />
          <stop offset="45%" stopColor="#ffb347" stopOpacity="0.85" />
          <stop offset="100%" stopColor="#ff7500" stopOpacity="0" />
        </radialGradient>
      </defs>
      {SPARKS.map((spark, i) => (
        <circle
          key={i}
          className="boot-svg-spark"
          cx={spark.cx}
          cy={spark.cy}
          r={spark.r}
          fill="url(#boot-spark)"
          style={{ animationDelay: spark.delay }}
        />
      ))}
    </svg>
  );
}
