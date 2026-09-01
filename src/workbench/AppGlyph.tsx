/** Reuse bundled logo silhouettes with readable, theme-aware app colors. */
export function AppGlyph({ source }: { source: string }) {
  return (
    <span
      className="application-glyph"
      aria-hidden="true"
      style={{
        maskImage: `url("${source}")`,
        WebkitMaskImage: `url("${source}")`,
      }}
    />
  );
}
