import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { TrustAsk } from "./TrustAsk";

vi.mock("./sound", async (original) => ({
  ...(await original<typeof import("./sound")>()),
  prefersCalm: () => true,
}));

describe("第一次见的站点先问一句", () => {
  it("没有要问的站点时不显示", () => {
    render(<TrustAsk origin={null} onAnswer={vi.fn()} />);
    expect(screen.getByTestId("kaifan-trust")).toHaveAttribute(
      "aria-hidden",
      "true",
    );
  });

  it("写出是哪个站；焦点停在「不连接」", () => {
    render(<TrustAsk origin="https://new.example.com" onAnswer={vi.fn()} />);
    expect(screen.getByRole("alertdialog")).toHaveTextContent(
      "new.example.com",
    );
    expect(screen.getByRole("button", { name: "不连接" })).toHaveFocus();
  });

  it("Esc 等于不连接", async () => {
    const onAnswer = vi.fn().mockResolvedValue(undefined);
    render(<TrustAsk origin="https://new.example.com" onAnswer={onAnswer} />);
    fireEvent.keyDown(screen.getByTestId("kaifan-trust"), { key: "Escape" });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledWith(false));
  });

  it("点了「我认识」才算同意", async () => {
    const onAnswer = vi.fn().mockResolvedValue(undefined);
    render(<TrustAsk origin="https://new.example.com" onAnswer={onAnswer} />);
    fireEvent.click(screen.getByRole("button", { name: /我认识/ }));
    await waitFor(() => expect(onAnswer).toHaveBeenCalledWith(true));
    expect(onAnswer).toHaveBeenCalledTimes(1);
  });

  it("没答成：话留在这一屏里说，按钮还能再点", async () => {
    const onAnswer = vi
      .fn()
      .mockResolvedValueOnce("再点一次。")
      .mockResolvedValue(undefined);
    render(<TrustAsk origin="https://new.example.com" onAnswer={onAnswer} />);
    fireEvent.click(screen.getByRole("button", { name: /我认识/ }));
    expect(await screen.findByTestId("kaifan-trust-retry")).toHaveTextContent(
      "再点一次。",
    );
    fireEvent.click(screen.getByRole("button", { name: /我认识/ }));
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(screen.queryByTestId("kaifan-trust-retry")).toBeNull(),
    );
  });
});
