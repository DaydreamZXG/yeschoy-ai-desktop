import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { createAppearanceSync } from "./nativeAppearance";
import { APPEARANCE_KEY, useAppearance } from "./appearance";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const native = vi.mocked(invoke);

const tick = async () => {
  await act(async () => {
    await Promise.resolve();
  });
};

let systemDark = false;
const listeners = new Set<() => void>();

beforeEach(() => {
  localStorage.removeItem(APPEARANCE_KEY);
  native.mockReset();
  native.mockResolvedValue(undefined);
  systemDark = false;
  listeners.clear();
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({
      get matches() {
        return systemDark;
      },
      addEventListener: (_: string, fn: () => void) => listeners.add(fn),
      removeEventListener: (_: string, fn: () => void) => listeners.delete(fn),
    })),
  );
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("native appearance ordering", () => {
  it("serializes assignments and coalesces rapid choices to latest", async () => {
    const completions: (() => void)[] = [];
    const apply = vi.fn(
      () => new Promise<void>((resolve) => completions.push(resolve)),
    );
    const sync = createAppearanceSync(apply);
    sync("dark");
    sync("light");
    sync("dark");
    sync("light");
    expect(apply.mock.calls).toEqual([["dark"]]);
    completions.shift()!();
    await tick();
    expect(apply.mock.calls).toEqual([["dark"], ["light"]]);
    completions.shift()!();
    await tick();
    sync("dark");
    expect(apply.mock.calls).toHaveLength(3);
    completions.shift()!();
    await tick();
  });

  it("a rejected native assignment does not prevent the next change", async () => {
    const apply = vi
      .fn()
      .mockRejectedValueOnce(Error("unavailable"))
      .mockResolvedValue(undefined);
    const sync = createAppearanceSync(apply);
    sync("dark");
    sync("light");
    await tick();
    expect(apply.mock.calls).toEqual([["dark"], ["light"]]);
  });

  it("syncs fixed palette and follows real system changes after leaving manual mode", async () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    function Harness() {
      const { changeAppearance } = useAppearance();
      return (
        <>
          {(["light", "dark", "system"] as const).map((value) => (
            <button key={value} onClick={() => changeAppearance(value)}>
              {value}
            </button>
          ))}
        </>
      );
    }
    const mounted = render(<Harness />);
    await tick();
    fireEvent.click(screen.getByText("dark"));
    await tick();
    expect(document.documentElement.dataset.theme).toBe("dark");
    fireEvent.click(screen.getByText("system"));
    await tick();
    expect(document.documentElement.dataset.theme).toBe("light");
    systemDark = true;
    act(() => listeners.forEach((fn) => fn()));
    await tick();
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(native.mock.calls.map(([cmd, args]) => [cmd, args])).toEqual(
      ["light", "dark", "light", "dark"].map((theme) => [
        "set_window_appearance",
        { request: { theme } },
      ]),
    );
    mounted.unmount();
    expect(listeners.size).toBe(0);
  });
});
