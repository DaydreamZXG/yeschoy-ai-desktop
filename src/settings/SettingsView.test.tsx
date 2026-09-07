import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
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
  });
});
