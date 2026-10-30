import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";
import { AppErrorBoundary } from "./AppErrorBoundary";

function Explodes(): ReactNode {
  throw new Error("render exploded");
}

describe("AppErrorBoundary", () => {
  afterEach(() => vi.restoreAllMocks());

  it("replaces a crashed subtree with the localized fallback", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    render(
      <AppErrorBoundary>
        <Explodes />
      </AppErrorBoundary>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("这个界面没能继续显示");
    expect(
      screen.getByRole("button", { name: "重新加载" }),
    ).toBeInTheDocument();
  });

  it("calls the provided reload hook instead of reloading the window", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const onReload = vi.fn();
    render(
      <AppErrorBoundary onReload={onReload}>
        <Explodes />
      </AppErrorBoundary>,
    );
    await userEvent.click(screen.getByRole("button", { name: "重新加载" }));
    expect(onReload).toHaveBeenCalledTimes(1);
  });

  it("renders children untouched when nothing throws", () => {
    render(
      <AppErrorBoundary>
        <p>all good</p>
      </AppErrorBoundary>,
    );
    expect(screen.getByText("all good")).toBeInTheDocument();
  });
});
