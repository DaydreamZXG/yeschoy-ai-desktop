import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// 这个产品是给「被配置劝退的人」用的。字号和间距一旦重新失控，
// 界面就会退回那种谁也读不下去的密度 —— 之前就发生过一次：
// 有一轮「压密度」改造把步骤标题压到 11px、副标题直接 display: none。
// 所以把下限写成测试，而不是写成注释。
const SHEETS = [
  "src/index.css",
  "src/tokens.css",
  "src/workbench/workbench-v2.css",
  "src/components/ui/ui-primitives.css",
  "src/components/ui/dialog.css",
  "src/installation/installation.css",
];

const sheets = SHEETS.map((path) => ({ path, css: readFileSync(path, "utf8") }));

const SPACING_PROPERTIES =
  "padding|margin|gap|row-gap|column-gap|padding-top|padding-bottom|" +
  "padding-left|padding-right|padding-block|padding-inline|margin-top|" +
  "margin-bottom|margin-left|margin-right";

describe("design system floors", () => {
  it("defines the full type scale and space ladder", () => {
    const tokens = readFileSync("src/tokens.css", "utf8");
    for (const step of ["xs", "sm", "base", "lg", "xl", "2xl", "3xl", "4xl"])
      expect(tokens).toContain(`--text-${step}:`);
    for (const step of [2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 24, 28, 32, 40])
      expect(tokens).toContain(`--space-${step}: ${step}px;`);
    // 下限就是 12px，别的步级都比它大。
    expect(tokens).toContain("--text-xs: 12px;");
  });

  it("never sets a font size below 12px", () => {
    const offenders: string[] = [];
    for (const { path, css } of sheets) {
      for (const match of css.matchAll(/font(?:-size)?:[^;]*?\b(\d+)px\b/g)) {
        if (Number(match[1]) < 12) offenders.push(`${path}: ${match[0]}`);
      }
    }
    expect(offenders).toEqual([]);
  });

  it("puts every font size on the scale rather than a literal px value", () => {
    const offenders: string[] = [];
    for (const { path, css } of sheets) {
      for (const match of css.matchAll(/^\s*font(?:-size)?:[^;]*;/gm)) {
        // clamp() 的上界允许是 token；剩下的字面 px 一律不允许。
        if (/\b\d+px\b/.test(match[0]) && !/line-height/.test(match[0]))
          offenders.push(`${path}: ${match[0].trim()}`);
      }
    }
    expect(offenders).toEqual([]);
  });

  it("keeps spacing on the ladder", () => {
    // 三处例外全是发丝线或视觉微调，不是节奏：
    // margin-top: 1px（三个图标的基线对齐）、margin: -1px（.sr-only）、
    // margin-left: 72px（一个断点里的对齐偏移）。
    const allowed = new Set(["1", "72"]);
    const offenders: string[] = [];
    for (const { path, css } of sheets) {
      const pattern = new RegExp(`^\\s*(?:${SPACING_PROPERTIES}):([^;]*);`, "gm");
      for (const match of css.matchAll(pattern)) {
        for (const value of match[1].matchAll(/(?<![-\w.])(\d+)px\b/g)) {
          if (!allowed.has(value[1]))
            offenders.push(`${path}: ${match[0].trim()}`);
        }
      }
    }
    expect(offenders).toEqual([]);
  });
});

describe("the setup steps stay legible", () => {
  const css = readFileSync("src/workbench/workbench-v2.css", "utf8");
  // 这张表是平铺的，同一个选择器会被后面的规则覆盖，
  // 所以取最后一条（层叠里真正生效的那条），并且必须整行匹配 ——
  // 否则 `.network-choice.connection-choice-card` 会被当成
  // `.connection-choice-card` 命中。
  const rule = (selector: string) => {
    const pattern = new RegExp(
      `^${selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")} \\{`,
      "gm",
    );
    const hits = [...css.matchAll(pattern)];
    expect(hits.length, `${selector} is missing`).toBeGreaterThan(0);
    const at = hits[hits.length - 1].index!;
    return css.slice(at, css.indexOf("}", at));
  };

  it("draws each step as a real card", () => {
    const card = rule(".connection-choice-card");
    expect(card).toContain("border: 1px solid var(--line)");
    expect(card).toContain("background: var(--surface)");
    expect(card).not.toContain("border: 0");
  });

  it("shows the sentence explaining what each step does", () => {
    const heading = rule(".choice-card-heading p");
    expect(heading).toContain("display: block");
    expect(heading).not.toContain("display: none");
  });

  it("makes the one primary action bigger than everything around it", () => {
    const apply = rule(".configuration-preview-panel .setup-apply");
    expect(apply).toContain("font-size: var(--text-lg)");
    expect(apply).toMatch(/min-height:\s*4[4-9]px/);
  });
});
