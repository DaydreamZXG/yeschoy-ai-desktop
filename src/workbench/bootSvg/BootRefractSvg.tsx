/**
 * Ignite burst — chromatic slash star, not citrus petals.
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
        <radialGradient id="fc-burst" cx="50%" cy="50%" r="50%">
          <stop offset="0%" stopColor="#ffffff" stopOpacity="0.85" />
          <stop offset="32%" stopColor="#c9fa10" stopOpacity="0.45" />
          <stop offset="100%" stopColor="#c9fa10" stopOpacity="0" />
        </radialGradient>
      </defs>
      <circle cx="100" cy="100" r="88" fill="url(#fc-burst)" />
      <path
        d="M 18 142 L 182 48"
        fill="none"
        stroke="#ff3df0"
        strokeWidth="10"
        strokeLinecap="square"
        opacity="0.75"
      />
      <path
        d="M 28 158 L 190 62"
        fill="none"
        stroke="#1cf0ff"
        strokeWidth="10"
        strokeLinecap="square"
        opacity="0.7"
      />
      <path
        d="M 22 150 L 186 54"
        fill="none"
        stroke="#c9fa10"
        strokeWidth="7"
        strokeLinecap="square"
      />
      <path
        d="M 100 18 L 108 86 L 182 100 L 108 114 L 100 182 L 92 114 L 18 100 L 92 86 Z"
        fill="#ffffff"
        fillOpacity="0.55"
      />
      <circle
        className="boot-svg-refract-ring boot-svg-refract-ring-1"
        cx="100"
        cy="100"
        r="36"
        fill="none"
        stroke="#c9fa10"
        strokeWidth="2"
      />
      <circle
        className="boot-svg-refract-ring boot-svg-refract-ring-2"
        cx="100"
        cy="100"
        r="58"
        fill="none"
        stroke="#ff3df0"
        strokeOpacity="0.7"
        strokeWidth="1.5"
      />
      <circle
        className="boot-svg-refract-ring boot-svg-refract-ring-3"
        cx="100"
        cy="100"
        r="82"
        fill="none"
        stroke="#1cf0ff"
        strokeOpacity="0.55"
        strokeWidth="1.2"
      />
    </svg>
  );
}
