/**
 * Full-bleed aurora veil — mist ribbons and moon rings over the character stage.
 */

const STAGE = { x: 1080, y: 400 };

const RINGS = [120, 220, 340, 480];

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
        <radialGradient id="th-veil-stage" cx="76%" cy="44%" r="46%">
          <stop offset="0%" stopColor="#ffffff" stopOpacity="0.42" />
          <stop offset="48%" stopColor="#c5b8ff" stopOpacity="0.22" />
          <stop offset="100%" stopColor="#6d5cff" stopOpacity="0" />
        </radialGradient>
        <linearGradient id="th-veil-iris" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="#6d5cff" stopOpacity="0" />
          <stop offset="50%" stopColor="#6d5cff" stopOpacity="0.28" />
          <stop offset="100%" stopColor="#6d5cff" stopOpacity="0" />
        </linearGradient>
        <linearGradient id="th-veil-sakura" x1="1" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#ff7eb3" stopOpacity="0" />
          <stop offset="50%" stopColor="#ff7eb3" stopOpacity="0.26" />
          <stop offset="100%" stopColor="#ff7eb3" stopOpacity="0" />
        </linearGradient>
        <linearGradient id="th-veil-aqua" x1="0" y1="1" x2="1" y2="0">
          <stop offset="0%" stopColor="#7ee8f4" stopOpacity="0" />
          <stop offset="50%" stopColor="#7ee8f4" stopOpacity="0.22" />
          <stop offset="100%" stopColor="#7ee8f4" stopOpacity="0" />
        </linearGradient>
      </defs>

      <ellipse
        className="boot-svg-stage-glow"
        cx={STAGE.x}
        cy={STAGE.y}
        rx="420"
        ry="520"
        fill="url(#th-veil-stage)"
      />

      <path
        className="boot-svg-ribbon"
        d="M -40 220 C 280 80, 620 340, 980 210 S 1420 80, 1500 160"
        fill="none"
        stroke="url(#th-veil-iris)"
        strokeWidth="90"
        strokeLinecap="round"
      />
      <path
        className="boot-svg-ribbon"
        d="M -60 520 C 300 640, 640 380, 1020 560 S 1400 720, 1520 600"
        fill="none"
        stroke="url(#th-veil-sakura)"
        strokeWidth="70"
        strokeLinecap="round"
      />
      <path
        className="boot-svg-ribbon"
        d="M -20 80 C 360 180, 700 40, 1100 140 S 1460 40, 1540 90"
        fill="none"
        stroke="url(#th-veil-aqua)"
        strokeWidth="52"
        strokeLinecap="round"
      />

      {RINGS.map((radius, index) => (
        <circle
          key={radius}
          className={`boot-svg-moonring boot-svg-moonring-${index + 1}`}
          cx={STAGE.x}
          cy={STAGE.y}
          r={radius}
          fill="none"
          stroke="#ffffff"
          strokeWidth={index === 0 ? 1.6 : 1}
          strokeOpacity={0.42 - index * 0.07}
        />
      ))}
    </svg>
  );
}
