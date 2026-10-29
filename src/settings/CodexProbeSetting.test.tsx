import { afterEach, describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import "../i18n";
import {
  readCodexProbeEnabled,
  writeCodexProbeEnabled,
} from "../configuration/codexProbe";
import { CodexProbeSetting } from "./CodexProbeSetting";

afterEach(() => {
  localStorage.clear();
});

describe("Codex transport probe setting", () => {
  it("starts off, says what turning it on costs, and persists the choice", async () => {
    const user = userEvent.setup();
    render(<CodexProbeSetting />);
    const toggle = screen.getByRole("button", {
      name: /已关闭/,
    });
    expect(toggle).toHaveAttribute("aria-pressed", "false");
    expect(screen.queryByTestId("codex-probe-warning")).toBeNull();

    await user.click(toggle);
    const on = screen.getByRole("button", { name: /已开启/ });
    expect(on).toHaveAttribute("aria-pressed", "true");
    // 开启后必须把「会产生少量用量」明说出来，否则用户不知道自己
    // 让渡了什么。
    expect(screen.getByTestId("codex-probe-warning")).toBeInTheDocument();
    expect(readCodexProbeEnabled()).toBe(true);

    await user.click(on);
    expect(screen.getByRole("button", { name: /已关闭/ })).toBeInTheDocument();
    expect(readCodexProbeEnabled()).toBe(false);
  });

  it("reflects a previously saved choice when the page opens", () => {
    writeCodexProbeEnabled(true);
    render(<CodexProbeSetting />);
    expect(screen.getByRole("button", { name: /已开启/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });
});
