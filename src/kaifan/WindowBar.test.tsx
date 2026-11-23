import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { WindowBar } from "./WindowBar";

const native = vi.hoisted(() => ({
  minimize: vi.fn(async () => undefined),
  toggleMaximize: vi.fn(async () => undefined),
  close: vi.fn(async () => undefined),
  startDragging: vi.fn(async () => undefined),
  isMaximized: vi.fn(async () => false),
  onResized: vi.fn(async () => () => undefined),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => native,
}));

beforeEach(() => {
  native.isMaximized.mockResolvedValue(false);
});

describe("没有系统标题栏的窗口", () => {
  it("自己画的三个按钮各管一件事", () => {
    const asked = vi.fn();
    window.addEventListener("kaifan:open-exit-choice", asked);
    render(<WindowBar frame="own" />);
    fireEvent.click(screen.getByRole("button", { name: "最小化" }));
    fireEvent.click(screen.getByRole("button", { name: "最大化" }));
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    expect(native.minimize).toHaveBeenCalledTimes(1);
    expect(native.toggleMaximize).toHaveBeenCalledTimes(1);
    // 关闭问的是界面里的退出框，不是系统关窗。
    expect(native.close).not.toHaveBeenCalled();
    expect(asked).toHaveBeenCalledTimes(1);
    window.removeEventListener("kaifan:open-exit-choice", asked);
  });

  it("窗口已经最大化时，中间那个按钮写的是还原", async () => {
    native.isMaximized.mockResolvedValue(true);
    render(<WindowBar frame="own" />);
    expect(await screen.findByRole("button", { name: "还原" })).toBeVisible();
  });

  it("macOS 用系统的三个圆点，不画自己的按钮", () => {
    render(<WindowBar frame="mac" />);
    expect(screen.queryByTestId("kaifan-window-controls")).toBeNull();
  });

  it("在最上面一条按住移动才拖动窗口，只是点一下不算", () => {
    render(
      <>
        <button type="button">角色</button>
        <WindowBar frame="mac" />
      </>,
    );
    const strip = screen.getByRole("button", { name: "角色" });
    fireEvent.mouseDown(strip, { button: 0, clientX: 300, clientY: 12 });
    fireEvent.mouseUp(strip, { clientX: 300, clientY: 12 });
    fireEvent.mouseMove(strip, { buttons: 0, clientX: 340, clientY: 30 });
    expect(native.startDragging).not.toHaveBeenCalled();

    fireEvent.mouseDown(strip, { button: 0, clientX: 300, clientY: 12 });
    fireEvent.mouseMove(strip, { buttons: 1, clientX: 302, clientY: 12 });
    expect(native.startDragging).not.toHaveBeenCalled();
    fireEvent.mouseMove(strip, { buttons: 1, clientX: 320, clientY: 20 });
    expect(native.startDragging).toHaveBeenCalledTimes(1);
  });

  it("在下面的画面里按住移动不拖动窗口", () => {
    render(<WindowBar frame="own" />);
    fireEvent.mouseDown(document.body, {
      button: 0,
      clientX: 300,
      clientY: 200,
    });
    fireEvent.mouseMove(document.body, {
      buttons: 1,
      clientX: 340,
      clientY: 240,
    });
    expect(native.startDragging).not.toHaveBeenCalled();
  });

  it("双击最上面的空白处放大窗口，双击按钮和输入框不算", () => {
    render(
      <>
        <button type="button">角色</button>
        <input aria-label="网址" />
        <WindowBar frame="own" />
      </>,
    );
    fireEvent.doubleClick(screen.getByRole("button", { name: "角色" }), {
      clientY: 10,
    });
    fireEvent.doubleClick(screen.getByLabelText("网址"), { clientY: 10 });
    expect(native.toggleMaximize).not.toHaveBeenCalled();
    fireEvent.doubleClick(document.body, { clientY: 10 });
    expect(native.toggleMaximize).toHaveBeenCalledTimes(1);
  });
});
