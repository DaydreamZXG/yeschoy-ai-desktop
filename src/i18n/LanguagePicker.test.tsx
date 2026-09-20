import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import i18n from "i18next";
import { useTranslation } from "react-i18next";

import { LanguagePicker } from "./LanguagePicker";
import { LANGUAGE_KEY, LANGUAGE_NAME, readLanguage } from "./language";
import { connectionLabel } from "../configuration/connections";

afterEach(async () => {
  // 语言是全局的。不还原的话，这个文件后面的用例会跑在英文界面上。
  await act(async () => {
    await i18n.changeLanguage("zh");
  });
  localStorage.clear();
});

/** 走 i18next 单例 `t` 的那类模块级函数，由一个订阅了语言的组件渲染。 */
function LabelProbe() {
  useTranslation();
  return <p data-testid="probe">{connectionLabel("connected")}</p>;
}

describe("界面语言切换器", () => {
  it("两个选项都用自己的语言写，任何界面语言下都一样", async () => {
    render(<LanguagePicker />);
    for (const name of Object.values(LANGUAGE_NAME))
      expect(screen.getByText(name)).toBeInTheDocument();

    // 这是这个组件存在的理由：看不懂当前语言的人也要认得出自己那个。
    await act(async () => {
      fireEvent.click(screen.getByText(LANGUAGE_NAME.en));
    });
    for (const name of Object.values(LANGUAGE_NAME))
      expect(screen.getByText(name)).toBeInTheDocument();
  });

  it("当前语言是按下状态", () => {
    render(<LanguagePicker />);
    expect(screen.getByRole("button", { pressed: true })).toHaveTextContent(
      LANGUAGE_NAME.zh,
    );
  });

  it("切换会落盘、改 html lang，并且立刻改变界面", async () => {
    render(<LanguagePicker />);
    await act(async () => {
      fireEvent.click(screen.getByText(LANGUAGE_NAME.en));
    });

    expect(i18n.language).toBe("en");
    expect(localStorage.getItem(LANGUAGE_KEY)).toBe("en");
    expect(readLanguage()).toBe("en");
    expect(document.documentElement.lang).toBe("en");
    // 说明文字本身也跟着换了，而不是只改了 i18n 的状态。
    expect(
      screen.getByText(/saved on this computer only/i),
    ).toBeInTheDocument();
  });

  it("走 i18next 单例的模块级函数也跟着换", async () => {
    // `connectionLabel` / `installationStage` 这类不是组件，拿不到 hook，用的是
    // 单例 `t`，自己不重渲染。它们全部由调了 `useTranslation()` 的组件渲染 ——
    // 这条把「所以没问题」这句推理变成会红的断言。
    render(
      <>
        <LanguagePicker />
        <LabelProbe />
      </>,
    );
    expect(screen.getByTestId("probe")).toHaveTextContent("已接入");

    await act(async () => {
      fireEvent.click(screen.getByText(LANGUAGE_NAME.en));
    });
    expect(screen.getByTestId("probe")).toHaveTextContent("Connected");
  });
});
