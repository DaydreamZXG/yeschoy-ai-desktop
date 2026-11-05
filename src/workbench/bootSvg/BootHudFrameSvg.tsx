/**
 * Soft corner HUD brackets + micro ticks behind / around the frosted card.
 * Decorative only — does not affect layout hit targets.
 */
export function BootHudFrameSvg() {
  return (
    <svg
      className="boot-svg-hud-frame"
      data-testid="boot-svg-hud-frame"
      viewBox="0 0 560 320"
      preserveAspectRatio="none"
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        <linearGradient id="boot-hud-edge" x1="0%" y1="0%" x2="100%" y2="0%">
          <stop offset="0%" stopColor="#ff7500" stopOpacity="0" />
          <stop offset="35%" stopColor="#ff7500" stopOpacity="0.55" />
          <stop offset="65%" stopColor="#ffb347" stopOpacity="0.55" />
          <stop offset="100%" stopColor="#ff7500" stopOpacity="0" />
        </linearGradient>
      </defs>

      {/* Corner brackets */}
      <g
        className="boot-svg-brackets"
        fill="none"
        stroke="#ff7500"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <path d="M18 54 V22 H54" opacity="0.85" />
        <path d="M542 54 V22 H506" opacity="0.85" />
        <path d="M18 266 V298 H54" opacity="0.85" />
        <path d="M542 266 V298 H506" opacity="0.85" />
      </g>

      {/* Top / bottom soft rails */}
      <line
        className="boot-svg-rail boot-svg-rail-top"
        x1="72"
        y1="28"
        x2="488"
        y2="28"
        stroke="url(#boot-hud-edge)"
        strokeWidth="1"
      />
      <line
        className="boot-svg-rail boot-svg-rail-bot"
        x1="72"
        y1="292"
        x2="488"
        y2="292"
        stroke="url(#boot-hud-edge)"
        strokeWidth="1"
      />

      {/* Micro glyph ticks along top rail */}
      <g className="boot-svg-rail-ticks" stroke="#ff7500" strokeWidth="1.25" strokeLinecap="round">
        <line x1="120" y1="22" x2="120" y2="34" opacity="0.55" />
        <line x1="200" y1="24" x2="200" y2="32" opacity="0.4" />
        <line x1="280" y1="20" x2="280" y2="36" opacity="0.7" />
        <line x1="360" y1="24" x2="360" y2="32" opacity="0.4" />
        <line x1="440" y1="22" x2="440" y2="34" opacity="0.55" />
      </g>
    </svg>
  );
}
