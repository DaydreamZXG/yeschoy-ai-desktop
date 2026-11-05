/**
 * Full-bleed AMV veil — speed lines + chromatic light beams.
 */

const FOCUS = { x: 1120, y: 360 };

const SPEED_LINES = Array.from({ length: 44 }, (_, i) => {
  const angle = (i / 44) * Math.PI * 2 + 0.08;
  const wobble = ((i * 17) % 13) - 6;
  const inner = 64 + (i % 7) * 14;
  const outer = 860 + wobble * 10;
  return {
    x1: +(FOCUS.x + Math.cos(angle) * inner).toFixed(1),
    y1: +(FOCUS.y + Math.sin(angle) * inner).toFixed(1),
    x2: +(FOCUS.x + Math.cos(angle) * outer).toFixed(1),
    y2: +(FOCUS.y + Math.sin(angle) * outer).toFixed(1),
    w: i % 5 === 0 ? 2.8 : i % 3 === 0 ? 1.5 : 0.7,
    o: i % 4 === 0 ? 0.55 : 0.2,
  };
});

export function BootVeilSvg() {
  return (
    <svg
      className="boot-svg-veil"
      data-testid="boot-svg-veil"
      viewBox="0 0 1440 900"
      preserveAspectRatio="xMidYMid slice"
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        <linearGradient id="fc-veil-beam" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="#c9fa10" stopOpacity="0" />
          <stop offset="48%" stopColor="#c9fa10" stopOpacity="0.55" />
          <stop offset="100%" stopColor="#ffffff" stopOpacity="0" />
        </linearGradient>
        <linearGradient id="fc-veil-magenta" x1="0" y1="1" x2="1" y2="0">
          <stop offset="0%" stopColor="#ff3df0" stopOpacity="0" />
          <stop offset="50%" stopColor="#ff3df0" stopOpacity="0.42" />
          <stop offset="100%" stopColor="#ff3df0" stopOpacity="0" />
        </linearGradient>
        <linearGradient id="fc-veil-cyan" x1="1" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#1cf0ff" stopOpacity="0" />
          <stop offset="50%" stopColor="#1cf0ff" stopOpacity="0.38" />
          <stop offset="100%" stopColor="#1cf0ff" stopOpacity="0" />
        </linearGradient>
      </defs>

      <path
        className="boot-svg-beam"
        d="M -80 80 L 1520 540 L 1520 640 L -80 180 Z"
        fill="url(#fc-veil-beam)"
      />
      <path
        className="boot-svg-beam"
        d="M -60 220 L 1540 700 L 1540 760 L -60 280 Z"
        fill="url(#fc-veil-magenta)"
      />
      <path
        className="boot-svg-beam"
        d="M -40 -20 L 1500 420 L 1500 470 L -40 40 Z"
        fill="url(#fc-veil-cyan)"
      />

      {SPEED_LINES.map((line, index) => (
        <line
          key={index}
          className="boot-svg-speedline"
          x1={line.x1}
          y1={line.y1}
          x2={line.x2}
          y2={line.y2}
          stroke="#ffffff"
          strokeWidth={line.w}
          strokeOpacity={line.o}
          style={{ animationDelay: `${(index % 8) * 40}ms` }}
        />
      ))}
    </svg>
  );
}
