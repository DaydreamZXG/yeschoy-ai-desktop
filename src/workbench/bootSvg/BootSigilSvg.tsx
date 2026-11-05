/**
 * TSUKIHANA moon-bloom sigil — concentric rings, crescent, and petals.
 * Not a slash, citrus flower, or gunsight.
 */
export function BootSigilSvg() {
  return (
    <svg
      className="boot-svg-sigil"
      data-testid="boot-svg-sigil"
      viewBox="0 0 200 200"
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        <radialGradient id="th-sigil-core" cx="42%" cy="38%" r="58%">
          <stop offset="0%" stopColor="#fff7fb" stopOpacity="0.95" />
          <stop offset="55%" stopColor="#c5b8ff" stopOpacity="0.55" />
          <stop offset="100%" stopColor="#6d5cff" stopOpacity="0" />
        </radialGradient>
        <linearGradient id="th-sigil-ring" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="#ff7eb3" stopOpacity="0.15" />
          <stop offset="45%" stopColor="#6d5cff" stopOpacity="0.95" />
          <stop offset="100%" stopColor="#7ee8f4" stopOpacity="0.7" />
        </linearGradient>
        <linearGradient id="th-sigil-petal" x1="0.2" y1="0" x2="0.8" y2="1">
          <stop offset="0%" stopColor="#ff7eb3" stopOpacity="0.85" />
          <stop offset="100%" stopColor="#6d5cff" stopOpacity="0.35" />
        </linearGradient>
      </defs>
      <circle cx="100" cy="100" r="86" fill="url(#th-sigil-core)" />
      <circle
        className="boot-svg-ring boot-svg-ring-outer"
        cx="100"
        cy="100"
        r="78"
        fill="none"
        stroke="url(#th-sigil-ring)"
        strokeWidth="1.2"
        strokeDasharray="4 7"
        opacity="0.7"
      />
      <circle
        className="boot-svg-ring boot-svg-ring-mid"
        cx="100"
        cy="100"
        r="58"
        fill="none"
        stroke="#6d5cff"
        strokeWidth="1.4"
        strokeOpacity="0.55"
      />
      <circle
        className="boot-svg-ring boot-svg-ring-inner"
        cx="100"
        cy="100"
        r="36"
        fill="none"
        stroke="#ff7eb3"
        strokeWidth="1.1"
        strokeOpacity="0.7"
      />
      <path
        className="boot-svg-crescent"
        d="M 118 52 A 40 40 0 1 0 118 148 A 32 32 0 1 1 118 52 Z"
        fill="#f7f3ff"
        fillOpacity="0.92"
      />
      <g className="boot-svg-petals" fill="url(#th-sigil-petal)">
        <ellipse cx="100" cy="34" rx="7" ry="16" />
        <ellipse
          cx="148"
          cy="62"
          rx="7"
          ry="16"
          transform="rotate(60 148 62)"
        />
        <ellipse
          cx="148"
          cy="138"
          rx="7"
          ry="16"
          transform="rotate(120 148 138)"
        />
        <ellipse cx="100" cy="166" rx="7" ry="16" />
        <ellipse
          cx="52"
          cy="138"
          rx="7"
          ry="16"
          transform="rotate(60 52 138)"
        />
        <ellipse cx="52" cy="62" rx="7" ry="16" transform="rotate(120 52 62)" />
      </g>
      <circle className="boot-svg-seed" cx="100" cy="100" r="8" fill="#fff" />
      <circle cx="100" cy="100" r="3.6" fill="#6d5cff" />
    </svg>
  );
}
