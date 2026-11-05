/**
 * Full-stage lucent veil — citrus ribbons and canopy caustics.
 * Editorial light, not HUD brackets or tick rails.
 */
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
        <linearGradient id="qm-ribbon" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="#c9fa10" stopOpacity="0" />
          <stop offset="38%" stopColor="#c9fa10" stopOpacity="0.42" />
          <stop offset="68%" stopColor="#d4b36a" stopOpacity="0.28" />
          <stop offset="100%" stopColor="#c9fa10" stopOpacity="0" />
        </linearGradient>
        <linearGradient id="qm-ribbon-soft" x1="1" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#1c6b4a" stopOpacity="0" />
          <stop offset="50%" stopColor="#7db82a" stopOpacity="0.22" />
          <stop offset="100%" stopColor="#c9fa10" stopOpacity="0" />
        </linearGradient>
        <radialGradient id="qm-caustic" cx="72%" cy="28%" r="42%">
          <stop offset="0%" stopColor="#c9fa10" stopOpacity="0.22" />
          <stop offset="55%" stopColor="#e7f6a8" stopOpacity="0.08" />
          <stop offset="100%" stopColor="#c9fa10" stopOpacity="0" />
        </radialGradient>
      </defs>

      <ellipse cx="1100" cy="210" rx="380" ry="240" fill="url(#qm-caustic)" />
      <ellipse
        cx="280"
        cy="640"
        rx="260"
        ry="180"
        fill="#c9fa10"
        fillOpacity="0.05"
      />

      <path
        className="boot-svg-ribbon boot-svg-ribbon-a"
        d="M-60 170 C 240 40, 520 300, 820 190 S 1260 40, 1520 230"
        fill="none"
        stroke="url(#qm-ribbon)"
        strokeWidth="2.4"
        strokeLinecap="round"
      />
      <path
        className="boot-svg-ribbon boot-svg-ribbon-b"
        d="M-40 430 C 280 540, 560 250, 920 400 S 1280 580, 1500 390"
        fill="none"
        stroke="url(#qm-ribbon-soft)"
        strokeWidth="1.8"
        strokeLinecap="round"
      />
      <path
        className="boot-svg-ribbon boot-svg-ribbon-c"
        d="M 40 760 C 360 620, 680 840, 1000 690 S 1340 800, 1540 650"
        fill="none"
        stroke="url(#qm-ribbon)"
        strokeWidth="1.6"
        strokeLinecap="round"
      />
    </svg>
  );
}
