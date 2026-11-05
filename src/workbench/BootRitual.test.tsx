import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import zh from "../i18n/locales/zh.json";
import { BootRitual } from "./BootRitual";
import { BOOT_SESSION_KEY } from "./bootPrefs";

beforeEach(async () => {
  localStorage.clear();
  sessionStorage.clear();
  i18n.addResourceBundle("zh", "translation", zh, true, true);
  await i18n.changeLanguage("zh");
});

afterEach(() => {
  localStorage.clear();
  sessionStorage.clear();
});

describe("BootRitual", () => {
  it("SKIP dismisses the overlay and marks the session played", async () => {
    const onComplete = vi.fn();
    render(
      <BootRitual
        character="dafeiyu"
        onComplete={onComplete}
        disableAutoAdvance
      />,
    );
    expect(screen.getByTestId("boot-ritual")).toHaveAttribute(
      "data-stage",
      "1",
    );
    fireEvent.click(screen.getByTestId("boot-ritual-skip"));
    expect(sessionStorage.getItem(BOOT_SESSION_KEY)).toBe("1");
    await waitFor(() => expect(onComplete).toHaveBeenCalledTimes(1));
  });

  it("Esc skips", async () => {
    const onComplete = vi.fn();
    render(
      <BootRitual
        character="dafeiyu"
        onComplete={onComplete}
        disableAutoAdvance
      />,
    );
    fireEvent.keyDown(window, { key: "Escape" });
    expect(sessionStorage.getItem(BOOT_SESSION_KEY)).toBe("1");
    await waitFor(() => expect(onComplete).toHaveBeenCalledTimes(1));
  });

  it("BREACH advances from stage 2", () => {
    render(
      <BootRitual
        character="dafeiyu"
        onComplete={vi.fn()}
        initialStage={2}
        disableAutoAdvance
      />,
    );
    expect(screen.getByTestId("boot-ritual")).toHaveAttribute(
      "data-stage",
      "2",
    );
    fireEvent.click(screen.getByTestId("boot-ritual-breach"));
    expect(screen.getByTestId("boot-ritual")).toHaveAttribute(
      "data-stage",
      "3",
    );
  });

  it("Enter on stage 2 acts as BREACH", () => {
    render(
      <BootRitual
        character="claude"
        onComplete={vi.fn()}
        initialStage={2}
        disableAutoAdvance
      />,
    );
    fireEvent.keyDown(window, { key: "Enter" });
    expect(screen.getByTestId("boot-ritual")).toHaveAttribute(
      "data-stage",
      "3",
    );
  });
});
