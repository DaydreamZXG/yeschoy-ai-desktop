import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// 回归：切到「应用接入」时 App.tsx 会 focus 可见 h1；全局青柠 focus-ring
// 画在块级标题上会变成「标题外面一个大方框」。
const css = readFileSync("src/theme/system.css", "utf8");
const app = readFileSync("src/App.tsx", "utf8");

describe("页标题不画切页焦点框", () => {
  it("切页仍会把焦点放到可见 h1（读屏）", () => {
    expect(app).toMatch(/heading\.tabIndex = -1/);
    expect(app).toMatch(/heading\.focus\(\{ preventScroll: true \}\)/);
  });

  it("tabindex=-1 的标题不套青柠 focus-ring", () => {
    expect(css).toMatch(
      /:is\(h1, h2, h3, h4, h5, h6\)\[tabindex="-1"\]:focus-visible/,
    );
    const rule = css.slice(
      css.indexOf(':is(h1, h2, h3, h4, h5, h6)[tabindex="-1"]:focus'),
    );
    expect(rule).toMatch(/box-shadow:\s*none/);
    expect(rule).toMatch(/outline:\s*none/);
  });
});
