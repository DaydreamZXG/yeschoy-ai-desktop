/** Bundled brand artwork: never recolor it with the application's accent. */
export function AppGlyph({ source }: { source: string }) {
  return (
    <img
      className="application-glyph"
      src={source}
      alt=""
      aria-hidden="true"
    />
  );
}
