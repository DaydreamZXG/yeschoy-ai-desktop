import { act, fireEvent, render, screen } from "@testing-library/react";
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
  vi.useFakeTimers({ shouldAdvanceTime: true });
});

afterEach(() => {
  vi.useRealTimers();
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
    await act(async () => {
      await vi.advanceTimersByTimeAsync(500);
    });
    expect(onComplete).toHaveBeenCalledTimes(1);
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
    await act(async () => {
      await vi.advanceTimersByTimeAsync(500);
    });
    expect(onComplete).toHaveBeenCalledTimes(1);
  });

  it("BREACH advances from stage 2 after flash", async () => {
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
      "data-flashing",
      "true",
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(400);
    });
    expect(screen.getByTestId("boot-ritual")).toHaveAttribute(
      "data-stage",
      "3",
    );
  });

  it("Enter on stage 2 acts as BREACH", async () => {
    render(
      <BootRitual
        character="claude"
        onComplete={vi.fn()}
        initialStage={2}
        disableAutoAdvance
      />,
    );
    fireEvent.keyDown(window, { key: "Enter" });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(400);
    });
    expect(screen.getByTestId("boot-ritual")).toHaveAttribute(
      "data-stage",
      "3",
    );
  });

  it("renders progress pips for the active act", () => {
    render(
      <BootRitual
        character="dafeiyu"
        onComplete={vi.fn()}
        initialStage={3}
        disableAutoAdvance
      />,
    );
    const pips = document.querySelectorAll(".boot-ritual-progress-pip");
    expect(pips).toHaveLength(4);
    expect(pips[2]).toHaveAttribute("data-state", "active");
    expect(pips[0]).toHaveAttribute("data-state", "done");
    expect(pips[3]).toHaveAttribute("data-state", "todo");
  });

  it("auto-advances stage 2 via BREACH after the dwell timer", async () => {
    render(
      <BootRitual character="dafeiyu" onComplete={vi.fn()} initialStage={2} />,
    );
    expect(screen.getByTestId("boot-ritual")).toHaveAttribute(
      "data-stage",
      "2",
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5200);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(400);
    });
    expect(screen.getByTestId("boot-ritual")).toHaveAttribute(
      "data-stage",
      "3",
    );
  });

  it("shows product-voice copy on stage 4", () => {
    render(
      <BootRitual
        character="dafeiyu"
        onComplete={vi.fn()}
        initialStage={4}
        disableAutoAdvance
      />,
    );
    expect(screen.getByTestId("boot-ritual")).toHaveTextContent(
      "登录一次，接入你的 AI 应用",
    );
    expect(screen.getByTestId("boot-ritual")).toHaveTextContent(
      "Sign in once · Connect your AI apps",
    );
  });

  it("mounts FLASHCUT SVG layers (sigil, motes, veil, refract)", () => {
    render(
      <BootRitual
        character="dafeiyu"
        onComplete={vi.fn()}
        initialStage={2}
        disableAutoAdvance
      />,
    );
    expect(screen.getByTestId("boot-ritual")).toHaveAttribute(
      "data-visual",
      "flashcut",
    );
    expect(screen.getByTestId("boot-svg-sigil")).toBeInTheDocument();
    expect(screen.getByTestId("boot-svg-motes")).toBeInTheDocument();
    expect(screen.getByTestId("boot-svg-veil")).toBeInTheDocument();
    expect(screen.getByTestId("boot-svg-refract")).toHaveAttribute(
      "data-active",
      "false",
    );
  });

  it("activates SVG refract burst while 切入 flashes", async () => {
    render(
      <BootRitual
        character="dafeiyu"
        onComplete={vi.fn()}
        initialStage={2}
        disableAutoAdvance
      />,
    );
    fireEvent.click(screen.getByTestId("boot-ritual-breach"));
    expect(screen.getByTestId("boot-svg-refract")).toHaveAttribute(
      "data-active",
      "true",
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(400);
    });
    expect(screen.getByTestId("boot-svg-refract")).toHaveAttribute(
      "data-active",
      "false",
    );
  });
});
