/**
 * Expanding energy rings shown during BREACH chromatic flash.
 */
export function BootEnergyPulseSvg({ active }: { active: boolean }) {
  return (
    <svg
      className="boot-svg-energy"
      data-testid="boot-svg-energy"
      data-active={active ? "true" : "false"}
      viewBox="0 0 200 200"
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        <radialGradient id="boot-energy-fill" cx="50%" cy="50%" r="50%">
          <stop offset="0%" stopColor="#ff7500" stopOpacity="0.35" />
          <stop offset="55%" stopColor="#ffb347" stopOpacity="0.12" />
          <stop offset="100%" stopColor="#ff7500" stopOpacity="0" />
        </radialGradient>
      </defs>
      <circle
        className="boot-svg-energy-core"
        cx="100"
        cy="100"
        r="28"
        fill="url(#boot-energy-fill)"
      />
      <circle
        className="boot-svg-energy-ring boot-svg-energy-ring-1"
        cx="100"
        cy="100"
        r="36"
        fill="none"
        stroke="#ff7500"
        strokeWidth="2"
        strokeOpacity="0.8"
      />
      <circle
        className="boot-svg-energy-ring boot-svg-energy-ring-2"
        cx="100"
        cy="100"
        r="52"
        fill="none"
        stroke="#ffb347"
        strokeWidth="1.5"
        strokeOpacity="0.55"
      />
      <circle
        className="boot-svg-energy-ring boot-svg-energy-ring-3"
        cx="100"
        cy="100"
        r="72"
        fill="none"
        stroke="#ff3b5c"
        strokeWidth="1"
        strokeOpacity="0.35"
      />
    </svg>
  );
}
