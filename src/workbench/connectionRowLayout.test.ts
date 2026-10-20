import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// 回归：已接入卡片里长模型名（如 gpt-6.1-sol）一字一行、盖住「换模型与分组」。
// 根因是 main 列 minmax(0,1fr) 被右侧「打开使用」反馈挤到 0 宽。
const css = readFileSync("src/theme/system.css", "utf8");
const view = readFileSync("src/workbench/AppLibraryView.tsx", "utf8");

describe("已接入卡片布局", () => {
  it("中间列有最小宽度，不会被右侧操作挤成 0", () => {
    expect(css).toMatch(
      /\.connection-library > \.connection-row \{\s*grid-template-columns: 40px minmax\(11rem, 1fr\)/,
    );
  });
  it("模型名单行省略，不按字符折行", () => {
    const rule = css.slice(
      css.indexOf(
        ".connection-row .connection-model-switch .connection-model {",
      ),
    );
    expect(rule).toMatch(/white-space: nowrap/);
    expect(rule).toMatch(/text-overflow: ellipsis/);
  });
  it("打开反馈换到下一行，操作区可换行", () => {
    expect(css).toMatch(
      /\.connection-row-actions \.open-feedback \{\s*flex-basis: 100%/,
    );
    expect(css).toMatch(/flex-wrap: wrap/);
  });
  it("被省略的模型名可以悬停看全", () => {
    expect(view).toMatch(/title=\{connection\.modelId \|\| undefined\}/);
  });

  it("卡片上优先显示可读模型名，完整 ID 放在 title", () => {
    expect(view).toMatch(/modelDisplayName\(connection\.modelId\)/);
  });
});
