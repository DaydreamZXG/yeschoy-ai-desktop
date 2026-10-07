import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import i18n from "i18next";
import zh from "../i18n/locales/zh.json";
import { AnnouncementsView, noticeLine } from "./AnnouncementsView";
import type { Notice } from "./announcements";

const notice: Notice = {
  id: "guoqing-2026-welfare",
  title: "国庆福利活动开启！双重福利+限时低倍率",
  body: "国庆期间给大家安排 **双重福利**！\n\n**① 充值返现**\n💰 每充值满 **100 元送 10 元**\n落单的 ** 原样显示",
  severity: "info",
  publishedAtEpochMs: 0,
  expiresAtEpochMs: 0,
  banner: true,
  actionLabel: "",
};

beforeEach(async () => {
  await i18n.init({
    lng: "zh",
    fallbackLng: "zh",
    resources: { zh: { translation: zh } },
  });
});

describe("announcement text", () => {
  it("renders **bold** and nothing else as markup", () => {
    const { container } = render(<p>{noticeLine("a **b** <i>c</i> **d**")}</p>);
    expect(
      [...container.querySelectorAll("strong")].map((s) => s.textContent),
    ).toEqual(["b", "d"]);
    expect(container.querySelector("i")).toBeNull();
    expect(container).toHaveTextContent("a b <i>c</i> d");
  });

  it("keeps an unmatched ** as text", () => {
    const { container } = render(<p>{noticeLine("x **y** z ** w")}</p>);
    expect(container.querySelectorAll("strong")).toHaveLength(1);
    expect(container).toHaveTextContent("x y z ** w");
  });

  it("shows the body of a notice in one column", () => {
    const { container } = render(
      <AnnouncementsView
        notices={[notice]}
        loading={false}
        failed={false}
        onRetry={() => {}}
        unread={new Set()}
        onFollow={() => {}}
      />,
    );
    expect(container.firstElementChild).toHaveClass(
      "workspace",
      "announcements-view",
    );
    expect(screen.getByText("双重福利").tagName).toBe("STRONG");
    expect(screen.getByText("100 元送 10 元").tagName).toBe("STRONG");
    expect(screen.getByText(/落单的 \*\* 原样显示/)).toBeInTheDocument();
    // 空行不渲染成空段落。
    expect(container.querySelectorAll(".announcement-card p")).toHaveLength(4);
  });
});
