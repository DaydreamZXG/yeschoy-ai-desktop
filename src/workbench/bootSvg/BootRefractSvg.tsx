/**
 * Ignite burst — citrus segments flare outward like light through a cut lime.
 */
function polar(
  radius: number,
  deg: number,
  cx = 100,
  cy = 100,
): [number, number] {
  const rad = (deg * Math.PI) / 180;
  return [cx + radius * Math.cos(rad), cy + radius * Math.sin(rad)];
}

function flarePath(index: number): string {
  const start = index * 60 - 90 + 8;
  const end = index * 60 - 90 + 52;
  const [x0, y0] = polar(18, start);
  const [x1, y1] = polar(72, start);
  const [x2, y2] = polar(72, end);
  const [x3, y3] = polar(18, end);
  return `M ${x0.toFixed(1)} ${y0.toFixed(1)} L ${x1.toFixed(1)} ${y1.toFixed(1)} A 72 72 0 0 1 ${x2.toFixed(1)} ${y2.toFixed(1)} L ${x3.toFixed(1)} ${y3.toFixed(1)} Z`;
}

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
        <radialGradient id="qm-refract" cx="50%" cy="50%" r="50%">
          <stop offset="0%" stopColor="#ffffff" stopOpacity="0.55" />
          <stop offset="35%" stopColor="#c9fa10" stopOpacity="0.35" />
          <stop offset="100%" stopColor="#c9fa10" stopOpacity="0" />
        </radialGradient>
      </defs>
      <circle cx="100" cy="100" r="86" fill="url(#qm-refract)" />
      {Array.from({ length: 6 }, (_, index) => (
        <path
          key={index}
          className={`boot-svg-flare boot-svg-flare-${index + 1}`}
          d={flarePath(index)}
          fill="#c9fa10"
          fillOpacity="0.28"
          stroke="#c9fa10"
          strokeOpacity="0.7"
          strokeWidth="1.2"
        />
      ))}
      <circle
        className="boot-svg-refract-ring boot-svg-refract-ring-1"
        cx="100"
        cy="100"
        r="36"
        fill="none"
        stroke="#c9fa10"
        strokeWidth="1.6"
      />
      <circle
        className="boot-svg-refract-ring boot-svg-refract-ring-2"
        cx="100"
        cy="100"
        r="58"
        fill="none"
        stroke="#d4b36a"
        strokeOpacity="0.7"
        strokeWidth="1.2"
      />
      <circle
        className="boot-svg-refract-ring boot-svg-refract-ring-3"
        cx="100"
        cy="100"
        r="82"
        fill="none"
        stroke="#1c6b4a"
        strokeOpacity="0.35"
        strokeWidth="1"
      />
    </svg>
  );
}
