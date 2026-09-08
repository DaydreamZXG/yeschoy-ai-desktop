import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import zh from "../i18n/locales/zh.json";
import { SettingsView } from "./SettingsView";

vi.mock("./QuitAssistant", () => ({
  QuitAssistant: () => null,
}));

describe("SettingsView", () => {
  it("does not render the obsolete account-unavailable boundary", () => {
    const { container } = render(
      <SettingsView onOpenAccount={vi.fn()} onOpenDiagnostics={vi.fn()} />,
    );

    expect(container.querySelector(".settings-boundary")).toBeNull();
    expect(container).toHaveTextContent("yeschoySettings.candidateStage");
    expect(zh.yeschoySettings.candidateStage).toBe("桌面版");
    expect(zh.yeschoySettings.candidateStage).not.toContain("测试");
  });
});
