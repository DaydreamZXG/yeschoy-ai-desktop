import { invoke } from "@tauri-apps/api/core";

export type Palette = "light" | "dark";

// One in-flight assignment; intermediate choices coalesce, so an older native
// write cannot finish after a newer one (including StrictMode effect replays).
export function createAppearanceSync(
  apply: (theme: Palette) => Promise<unknown>,
) {
  let pending: Palette | undefined;
  let running = false;
  async function drain() {
    running = true;
    while (pending !== undefined) {
      const next = pending;
      pending = undefined;
      try {
        await apply(next);
      } catch {
        // A chrome failure must not block navigation or the next appearance change.
      }
    }
    running = false;
  }
  return (theme: Palette) => {
    pending = theme;
    if (!running) void drain();
  };
}

const sync = createAppearanceSync((theme) =>
  invoke("set_window_appearance", { request: { theme } }),
);

export function syncNativeAppearance(theme: Palette) {
  if ("__TAURI_INTERNALS__" in window) sync(theme);
}
