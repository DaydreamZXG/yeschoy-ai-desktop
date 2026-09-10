import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { StrictMode, useState } from "react";
import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  afterAll,
  afterEach,
  beforeAll,
  describe,
  expect,
  it,
  vi,
} from "vitest";
import { ConfirmDialog } from "./ConfirmDialog";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "./ui/dialog";

const style = document.createElement("style");
beforeAll(() => {
  // Vitest strips CSS imports. Exercise the actual component CSS in JSDOM,
  // not a mocked Dialog; browser/build checks separately verify the import.
  style.textContent = readFileSync(resolve(__dirname, "ui/dialog.css"), "utf8");
  document.head.append(style);
});
afterEach(cleanup);
afterAll(() => style.remove());

function Harness({ onConfirm = () => {} }: { onConfirm?: () => void }) {
  const [open, setOpen] = useState(false);
  const [pending, setPending] = useState(false);
  const [clicks, setClicks] = useState(0);
  return (
    <>
      <button onClick={() => setOpen(true)}>重新接入</button>
      <button onClick={() => setClicks((value) => value + 1)}>设置</button>
      <output data-testid="clicks">{clicks}</output>
      {pending && (
        <button onClick={() => setPending(false)}>完成模拟配置</button>
      )}
      <ConfirmDialog
        isOpen={open}
        title="保存后重新打开 Codex"
        message="请先保存内容。"
        variant="info"
        confirmText="已保存，继续"
        cancelText="取消接入"
        pending={pending}
        onCancel={() => setOpen(false)}
        onConfirm={() => {
          setOpen(false);
          setPending(true);
          onConfirm();
        }}
      />
    </>
  );
}

describe("workbench confirmation dialog layout and input recovery", () => {
  it("owns fixed viewport positioning even without Tailwind utilities", async () => {
    const user = userEvent.setup();
    render(
      <StrictMode>
        <Harness />
      </StrictMode>,
    );
    await user.click(screen.getByRole("button", { name: "重新接入" }));
    const dialog = screen.getByRole("dialog", { name: "保存后重新打开 Codex" });
    expect(getComputedStyle(dialog).position).toBe("fixed");
    expect(getComputedStyle(dialog).top).toBe("50%");
    expect(getComputedStyle(dialog).left).toBe("50%");
    expect(getComputedStyle(dialog).overflowY).toBe("auto");
    const overlay = document.querySelector(".yeschoy-dialog-overlay")!;
    expect(getComputedStyle(overlay).position).toBe("fixed");
    expect(dialog).toHaveAttribute("data-dialog-layer", "alert");
    expect(overlay).toHaveAttribute("data-dialog-layer", "alert");
    expect(dialog).toHaveAccessibleDescription("请先保存内容。");
    expect(getComputedStyle(document.body).pointerEvents).toBe("none");
  });

  it("releases input after cancel, Escape and confirm across repeated reconnects", async () => {
    const user = userEvent.setup();
    const confirmed = vi.fn();
    render(
      <StrictMode>
        <Harness onConfirm={confirmed} />
      </StrictMode>,
    );
    for (const action of ["cancel", "escape", "confirm", "cancel", "confirm"]) {
      await user.click(screen.getByRole("button", { name: "重新接入" }));
      const dialog = screen.getByRole("dialog");
      if (action === "escape") await user.keyboard("{Escape}");
      else
        await user.click(
          within(dialog).getByRole("button", {
            name: action === "confirm" ? "已保存，继续" : "取消接入",
          }),
        );
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      expect(document.querySelector(".yeschoy-dialog-overlay")).toBeNull();
      expect(getComputedStyle(document.body).pointerEvents).not.toBe("none");
      await user.click(screen.getByRole("button", { name: "设置" }));
      if (action === "confirm")
        await user.click(screen.getByRole("button", { name: "完成模拟配置" }));
    }
    expect(confirmed).toHaveBeenCalledTimes(2);
    expect(screen.getByTestId("clicks")).toHaveTextContent("5");
  });

  it.each(["base", "nested", "alert", "top"] as const)(
    "preserves the shared %s modal layer without utility styles",
    (layer) => {
      render(
        <Dialog open>
          <DialogContent zIndex={layer}>
            <DialogTitle>共享对话框</DialogTitle>
            <DialogDescription>说明</DialogDescription>
          </DialogContent>
        </Dialog>,
      );
      const dialog = screen.getByRole("dialog");
      expect(dialog).toHaveAttribute("data-dialog-layer", layer);
      expect(document.querySelector(".yeschoy-dialog-overlay")).toHaveAttribute(
        "data-dialog-layer",
        layer,
      );
      expect(getComputedStyle(dialog).position).toBe("fixed");
    },
  );

  it("keeps fullscreen dialogs in the viewport", () => {
    render(
      <Dialog open>
        <DialogContent variant="fullscreen">
          <DialogTitle>全屏对话框</DialogTitle>
          <DialogDescription>说明</DialogDescription>
        </DialogContent>
      </Dialog>,
    );
    expect(getComputedStyle(screen.getByRole("dialog")).transform).toBe("none");
    expect(getComputedStyle(screen.getByRole("dialog")).maxHeight).toBe("none");
  });
});
