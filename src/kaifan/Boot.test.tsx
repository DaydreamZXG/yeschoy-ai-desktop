import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Boot } from "./Boot";

const calm = vi.hoisted(() => ({ on: false }));
vi.mock("./sound", async (original) => ({
  ...(await original<typeof import("./sound")>()),
  prefersCalm: () => calm.on,
}));

describe("开机", () => {
  beforeEach(() => {
    calm.on = false;
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("放完才进首页", () => {
    const onDone = vi.fn();
    render(<Boot station="my.example.com" castCount={7} onDone={onDone} />);
    act(() => void vi.advanceTimersByTime(5600));
    expect(onDone).not.toHaveBeenCalled();
    act(() => void vi.advanceTimersByTime(200));
    expect(onDone).toHaveBeenCalledTimes(1);
  });

  it("知道多少说多少：有站点才说接通站点，有人数才报人数", () => {
    const { unmount } = render(
      <Boot station="my.example.com" castCount={7} onDone={vi.fn()} />,
    );
    act(() => void vi.advanceTimersByTime(800));
    expect(screen.getByTestId("kaifan-boot")).toHaveTextContent(
      "正在敲站点的门",
    );
    act(() => void vi.advanceTimersByTime(2000));
    expect(screen.getByTestId("kaifan-boot")).toHaveTextContent("7 位同学就位");
    unmount();

    render(<Boot station={null} castCount={null} onDone={vi.fn()} />);
    act(() => void vi.advanceTimersByTime(800));
    expect(screen.getByTestId("kaifan-boot")).toHaveTextContent("正在热身");
    expect(screen.getByTestId("kaifan-boot")).not.toHaveTextContent(
      "敲站点的门",
    );
    act(() => void vi.advanceTimersByTime(2000));
    expect(screen.getByTestId("kaifan-boot")).toHaveTextContent(
      "同学们正往这边赶",
    );
  });

  it("Esc 和「跳过」都能直接进首页", () => {
    const onDone = vi.fn();
    const { unmount } = render(
      <Boot station={null} castCount={null} onDone={onDone} />,
    );
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onDone).toHaveBeenCalledTimes(1);
    unmount();

    render(<Boot station={null} castCount={null} onDone={onDone} />);
    fireEvent.click(screen.getByText(/跳过/));
    expect(onDone).toHaveBeenCalledTimes(2);
  });

  it("系统里关了动效的人：不放，直接进首页", () => {
    calm.on = true;
    const onDone = vi.fn();
    render(<Boot station={null} castCount={null} onDone={onDone} />);
    expect(onDone).toHaveBeenCalledTimes(1);
  });
});
