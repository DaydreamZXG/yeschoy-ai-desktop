import "@testing-library/jest-dom";
import { afterAll, afterEach, beforeAll, vi } from "vitest";
import { cleanup } from "@testing-library/react";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { server } from "./msw/server";
import "./msw/tauriMocks";
import { queryClient } from "../src/lib/query/queryClient";
import en from "../src/i18n/locales/en.json";
import zh from "../src/i18n/locales/zh.json";

beforeAll(async () => {
  server.listen({ onUnhandledRequest: "warn" });
  await i18n.use(initReactI18next).init({
    lng: "zh",
    fallbackLng: "zh",
    // 真实的译文，和生产一致。以前这里是空的，各测试文件自己 `addResourceBundle`——
    // 文案从 `copy.ts` 迁进 i18n 之后，漏掉那一步的文件会渲染出键名，
    // 表现成「找不到这个按钮」，与真正的被测行为无关。
    resources: {
      zh: { translation: zh },
      en: { translation: en },
    },
    interpolation: {
      escapeValue: false,
    },
  });
});

afterEach(() => {
  cleanup();
  queryClient.clear();
  server.resetHandlers();
  vi.clearAllMocks();
});

afterAll(() => {
  server.close();
});
