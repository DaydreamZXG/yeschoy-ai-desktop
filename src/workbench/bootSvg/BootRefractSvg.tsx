/**
 * Unveil bloom — expanding glass rings and a six-petal burst.
 */
export function BootRefractSvg({ active }: { active: boolean }) {
  return (
    <svg
      className="boot-svg-refract"
      data-testid="boot-svg-refract"
      data-active={active ? "true" : "false"}
      viewBox="0 0 200 200"
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        <radialGradient id="th-burst" cx="50%" cy="50%" r="50%">
          <stop offset="0%" stopColor="#ffffff" stopOpacity="0.9" />
          <stop offset="34%" stopColor="#ff7eb3" stopOpacity="0.38" />
          <stop offset="100%" stopColor="#6d5cff" stopOpacity="0" />
        </radialGradient>
        <linearGradient id="th-burst-petal" x1="0.2" y1="0" x2="0.8" y2="1">
          <stop offset="0%" stopColor="#ffffff" stopOpacity="0.95" />
          <stop offset="100%" stopColor="#6d5cff" stopOpacity="0.35" />
        </linearGradient>
      </defs>
      <circle cx="100" cy="100" r="88" fill="url(#th-burst)" />
      <g className="boot-svg-refract-petals" fill="url(#th-burst-petal)">
        <ellipse cx="100" cy="46" rx="10" ry="28" />
        <ellipse
          cx="146"
          cy="72"
          rx="10"
          ry="28"
          transform="rotate(60 146 72)"
        />
        <ellipse
          cx="146"
          cy="128"
          rx="10"
          ry="28"
          transform="rotate(120 146 128)"
        />
        <ellipse cx="100" cy="154" rx="10" ry="28" />
        <ellipse
          cx="54"
          cy="128"
          rx="10"
          ry="28"
          transform="rotate(60 54 128)"
        />
        <ellipse
          cx="54"
          cy="72"
          rx="10"
          ry="28"
          transform="rotate(120 54 72)"
        />
      </g>
      <circle
        className="boot-svg-refract-ring boot-svg-refract-ring-1"
        cx="100"
        cy="100"
        r="36"
        fill="none"
        stroke="#6d5cff"
        strokeWidth="1.6"
      />
      <circle
        className="boot-svg-refract-ring boot-svg-refract-ring-2"
        cx="100"
        cy="100"
        r="58"
        fill="none"
        stroke="#ff7eb3"
        strokeOpacity="0.75"
        strokeWidth="1.3"
      />
      <circle
        className="boot-svg-refract-ring boot-svg-refract-ring-3"
        cx="100"
        cy="100"
        r="82"
        fill="none"
        stroke="#7ee8f4"
        strokeOpacity="0.55"
        strokeWidth="1.1"
      />
    </svg>
  );
}
