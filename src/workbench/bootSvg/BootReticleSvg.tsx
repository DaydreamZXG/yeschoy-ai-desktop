/**
 * Multi-ring orange reticle with scan arcs + glyph ticks.
 * Pure SVG/CSS; animations live in bootRitual.css.
 */
export function BootReticleSvg() {
  return (
    <svg
      className="boot-svg-reticle"
      data-testid="boot-svg-reticle"
      viewBox="0 0 96 96"
      width="56"
      height="56"
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        <linearGradient id="boot-reticle-stroke" x1="0%" y1="0%" x2="100%" y2="100%">
          <stop offset="0%" stopColor="#ffb347" stopOpacity="0.95" />
          <stop offset="55%" stopColor="#ff7500" stopOpacity="1" />
          <stop offset="100%" stopColor="#ff3b5c" stopOpacity="0.55" />
        </linearGradient>
      </defs>

      {/* Outer dashed scan ring */}
      <circle
        className="boot-svg-ring boot-svg-ring-outer"
        cx="48"
        cy="48"
        r="42"
        fill="none"
        stroke="url(#boot-reticle-stroke)"
        strokeWidth="1.25"
        strokeDasharray="4 7"
        strokeLinecap="round"
      />

      {/* Mid solid ring */}
      <circle
        className="boot-svg-ring boot-svg-ring-mid"
        cx="48"
        cy="48"
        r="30"
        fill="none"
        stroke="#ff7500"
        strokeOpacity="0.72"
        strokeWidth="1.5"
      />

      {/* Inner aim ring */}
      <circle
        className="boot-svg-ring boot-svg-ring-inner"
        cx="48"
        cy="48"
        r="14"
        fill="color-mix(in srgb, #ff7500 12%, transparent)"
        stroke="#ff7500"
        strokeOpacity="0.9"
        strokeWidth="1.75"
      />

      {/* Scan arcs (two opposing) */}
      <path
        className="boot-svg-scan-arc boot-svg-scan-arc-a"
        d="M48 8 A40 40 0 0 1 88 48"
        fill="none"
        stroke="#ff7500"
        strokeWidth="2.25"
        strokeLinecap="round"
        strokeOpacity="0.85"
      />
      <path
        className="boot-svg-scan-arc boot-svg-scan-arc-b"
        d="M48 88 A40 40 0 0 1 8 48"
        fill="none"
        stroke="#ffb347"
        strokeWidth="1.75"
        strokeLinecap="round"
        strokeOpacity="0.7"
      />

      {/* Crosshair ticks */}
      <g className="boot-svg-cross" stroke="#ff7500" strokeWidth="1.75" strokeLinecap="round">
        <line x1="48" y1="4" x2="48" y2="16" />
        <line x1="48" y1="80" x2="48" y2="92" />
        <line x1="4" y1="48" x2="16" y2="48" />
        <line x1="80" y1="48" x2="92" y2="48" />
      </g>

      {/* Glyph ticks around mid ring */}
      <g className="boot-svg-glyphs" stroke="#ff7500" strokeWidth="1.5" strokeLinecap="round">
        <line x1="48" y1="16" x2="48" y2="22" opacity="0.85" />
        <line x1="70" y1="26" x2="66" y2="30" opacity="0.7" />
        <line x1="80" y1="48" x2="74" y2="48" opacity="0.85" />
        <line x1="70" y1="70" x2="66" y2="66" opacity="0.7" />
        <line x1="48" y1="80" x2="48" y2="74" opacity="0.85" />
        <line x1="26" y1="70" x2="30" y2="66" opacity="0.7" />
        <line x1="16" y1="48" x2="22" y2="48" opacity="0.85" />
        <line x1="26" y1="26" x2="30" y2="30" opacity="0.7" />
      </g>

      {/* Center pip */}
      <circle className="boot-svg-core" cx="48" cy="48" r="3.2" fill="#ff7500" />
    </svg>
  );
}
