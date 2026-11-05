/**
 * FLASHCUT slash mark — a blade of light, not a citrus flower or gunsight.
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
        <linearGradient id="fc-sigil-slash" x1="0" y1="1" x2="1" y2="0">
          <stop offset="0%" stopColor="#c9fa10" stopOpacity="0.15" />
          <stop offset="42%" stopColor="#c9fa10" stopOpacity="1" />
          <stop offset="100%" stopColor="#ffffff" stopOpacity="0.95" />
        </linearGradient>
      </defs>
      <path
        className="boot-svg-slash boot-svg-slash-m"
        d="M 22 176 L 158 16"
        fill="none"
        stroke="#ff3df0"
        strokeWidth="22"
        strokeLinecap="square"
        opacity="0.7"
      />
      <path
        className="boot-svg-slash boot-svg-slash-c"
        d="M 38 182 L 176 24"
        fill="none"
        stroke="#1cf0ff"
        strokeWidth="22"
        strokeLinecap="square"
        opacity="0.65"
      />
      <path
        className="boot-svg-slash boot-svg-slash-main"
        d="M 28 178 L 168 20"
        fill="none"
        stroke="url(#fc-sigil-slash)"
        strokeWidth="14"
        strokeLinecap="square"
        strokeDasharray="280"
        strokeDashoffset="0"
      />
      <path
        d="M 118 18 L 156 162"
        fill="none"
        stroke="#ffffff"
        strokeWidth="3"
        strokeLinecap="square"
        opacity="0.85"
      />
      <circle className="boot-svg-seed" cx="138" cy="62" r="7" fill="#ffffff" />
      <circle cx="138" cy="62" r="3" fill="#c9fa10" />
    </svg>
  );
}
