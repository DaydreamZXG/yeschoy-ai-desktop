/**
 * Qingmang citrus sigil — six light-segments, not a gunsight.
 * Geometry is a lime cut open: wedges, a seed, a slow botanical orbit.
 */
function polar(radius: number, deg: number): [number, number] {
  const rad = (deg * Math.PI) / 180;
  return [60 + radius * Math.cos(rad), 60 + radius * Math.sin(rad)];
}

function segmentPath(index: number): string {
  const gap = 7;
  const start = index * 60 - 90 + gap / 2;
  const end = index * 60 - 90 + 60 - gap / 2;
  const inner = 16;
  const outer = 46;
  const [x0i, y0i] = polar(inner, start);
  const [x0o, y0o] = polar(outer, start);
  const [x1o, y1o] = polar(outer, end);
  const [x1i, y1i] = polar(inner, end);
  return [
    `M ${x0i.toFixed(2)} ${y0i.toFixed(2)}`,
    `L ${x0o.toFixed(2)} ${y0o.toFixed(2)}`,
    `A ${outer} ${outer} 0 0 1 ${x1o.toFixed(2)} ${y1o.toFixed(2)}`,
    `L ${x1i.toFixed(2)} ${y1i.toFixed(2)}`,
    `A ${inner} ${inner} 0 0 0 ${x0i.toFixed(2)} ${y0i.toFixed(2)}`,
    "Z",
  ].join(" ");
}

export function BootSigilSvg() {
  return (
    <svg
      className="boot-svg-sigil"
      data-testid="boot-svg-sigil"
      viewBox="0 0 120 120"
      width="128"
      height="128"
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        <linearGradient id="qm-petal" x1="50%" y1="0%" x2="50%" y2="100%">
          <stop offset="0%" stopColor="#f4ff9a" stopOpacity="0.95" />
          <stop offset="55%" stopColor="#c9fa10" stopOpacity="0.88" />
          <stop offset="100%" stopColor="#7db82a" stopOpacity="0.55" />
        </linearGradient>
        <linearGradient id="qm-orbit" x1="0%" y1="0%" x2="100%" y2="100%">
          <stop offset="0%" stopColor="#c9fa10" stopOpacity="0.15" />
          <stop offset="50%" stopColor="#c9fa10" stopOpacity="0.85" />
          <stop offset="100%" stopColor="#d4b36a" stopOpacity="0.2" />
        </linearGradient>
        <radialGradient id="qm-seed" cx="50%" cy="50%" r="50%">
          <stop offset="0%" stopColor="#ffffff" stopOpacity="0.95" />
          <stop offset="55%" stopColor="#c9fa10" stopOpacity="1" />
          <stop offset="100%" stopColor="#1c6b4a" stopOpacity="0.35" />
        </radialGradient>
      </defs>

      <circle
        className="boot-svg-orbit boot-svg-orbit-outer"
        cx="60"
        cy="60"
        r="54"
        fill="none"
        stroke="url(#qm-orbit)"
        strokeWidth="1.15"
        strokeDasharray="3 8"
        strokeLinecap="round"
      />
      <circle
        className="boot-svg-orbit boot-svg-orbit-inner"
        cx="60"
        cy="60"
        r="22"
        fill="none"
        stroke="#c9fa10"
        strokeOpacity="0.35"
        strokeWidth="1"
      />

      <g className="boot-svg-segments">
        {Array.from({ length: 6 }, (_, index) => (
          <path
            key={index}
            className="boot-svg-petal"
            d={segmentPath(index)}
            fill="url(#qm-petal)"
            fillOpacity="0.82"
            stroke="#1c6b4a"
            strokeOpacity="0.28"
            strokeWidth="0.8"
            style={{ animationDelay: `${index * 70}ms` }}
          />
        ))}
      </g>

      <circle
        className="boot-svg-seed"
        cx="60"
        cy="60"
        r="7.5"
        fill="url(#qm-seed)"
      />
      <circle cx="60" cy="60" r="3.2" fill="#1c6b4a" fillOpacity="0.55" />
    </svg>
  );
}
